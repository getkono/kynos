//! Middleware that declares what it does to the contract.
//!
//! # Why not `tower::Layer`
//!
//! A `Layer` can change the status, rewrite the body, add headers, or refuse
//! the request entirely, and the type system says nothing about which. Wrapping
//! an operation in one therefore invalidates its description in a way no tool
//! can detect. That is the single largest source of wrong OpenAPI documents in
//! the Rust ecosystem today.
//!
//! Kynos splits middleware in two:
//!
//! - An [`Interceptor`] can affect the exchange, and declares how in its own
//!   signature: the responses it can answer with, the headers it adds, and the
//!   headers it reads are three associated types, so what it says and what it
//!   does are the same text.
//! - An [`Observer`] sees everything and changes nothing, so it needs to
//!   declare nothing. Logging, tracing and metrics live here.
//!
//! This is stricter than tower, and also *more* useful: you can write your own
//! interceptor, and attaching it to a group documents its effect on every
//! operation underneath automatically. With tower that mapping is maintained by
//! hand, and drifts.
//!
//! The `unchecked` feature restores `Layer` support for anyone who needs it,
//! at the price of a description marked non-authoritative.
//!
//! # Wire-visible but contract-neutral
//!
//! Some headers are defined by HTTP itself and handled by every client without
//! being told: `Vary`, `Content-Encoding`, the CORS set. These are still
//! *declared* -- an interceptor cannot set a header it did not name, and two
//! interceptors naming one header do not compile -- but their group sets
//! [`HeaderParams::DESCRIBED`] to `false`, so they stay out of the emitted
//! description. Declaring and describing are separate questions, and only the
//! first is about correctness.
//!
//! [`HeaderParams::DESCRIBED`]: crate::extract::params::header::HeaderParams::DESCRIBED
//!
//! # How this module is laid out
//!
//! The two traits and [`Continued`] live here; every interceptor Kynos
//! ships has its own module. Adding one is a new file plus one `pub mod` line,
//! and the ones that need a feature are gated at that line rather than at each
//! item.

pub mod catch_panic;
pub mod contribution;
pub mod cors;
pub mod limits;
pub mod rate_limit;
pub mod request_id;
pub mod stack;

// Object-safe forms of the two RPITIT traits, so a heterogeneous chain fits in
// one collection. `pub(crate)` because the router holds the chain and runs it;
// never `pub`, so `Pin<Box<dyn Future>>` reaches no user signature.
pub(crate) mod erased;

#[cfg(feature = "compression")]
pub mod compression;
#[cfg(feature = "trace")]
pub mod trace;

use std::{future::Future, sync::Arc};

use crate::{
    extract::params::header::HeaderParams,
    http::{Request, Response},
    middleware::erased::{ErasedInterceptor, ErasedTerminal},
    response::ShortCircuit,
    router::operation::Route,
};

/// Middleware that can affect the exchange, and says how in its own signature.
///
/// There is no `contribution` method. What an interceptor declares and what it
/// does are the same text: each associated type is both the obligation and the
/// declaration, so an interceptor cannot say one thing and do another.
///
/// * [`Short`](Interceptor::Short) is the only way to answer without reaching
///   the handler, and its [`Responses`](crate::response::Responses) is what
///   the document prints. A 401
///   cannot be declared without a type carrying it, nor sent without declaring
///   it. Use [`Infallible`](std::convert::Infallible) to always continue.
/// * [`Adds`](Interceptor::Adds) is the response headers this interceptor
///   attaches. [`Next::run`] yields `Continued<()>` and
///   [`Continued::with_headers`] is the only way to reach `Continued<H>`, so
///   declaring headers and never attaching them does not compile, and
///   attaching undeclared ones has no method to call.
/// * [`Reads`](Interceptor::Reads) is the request headers it consumes, handed
///   over already extracted. An interceptor cannot declare a parameter it
///   never reads, because reading is how it gets one.
///
/// The `C: Sync + 'static` bound is stated once here rather than repeated on
/// every implementation: it is what makes [`Next`] `Send` unconditionally, so
/// no interceptor has to reason about whether its own future is.
///
/// # What is left undeclared
///
/// [`Continued::take_body`] and [`Continued::set_body`] rewrite a body without
/// declaring anything, because a body has no name to collide on and an encoding
/// a consumer must know about is a header. Injecting a route and retrying are not
/// expressible here at all: the first is what the `unchecked` escape hatches
/// are for, and the second is invisible in any single response. See
/// [`docs/middleware.md`] for the invariant this buys and the one it does not.
///
/// [`docs/middleware.md`]: https://github.com/getkono/kynos/blob/master/docs/middleware.md
pub trait Interceptor<C: Sync + 'static>: Send + Sync + 'static {
    /// Request headers this interceptor reads, and therefore declares.
    ///
    /// `()` when it reads none.
    type Reads: HeaderParams + Send;

    /// Response headers this interceptor adds to a forwarded response.
    ///
    /// `()` when it adds none.
    type Adds: HeaderParams;

    /// Responses this interceptor produces without reaching the handler.
    ///
    /// [`Infallible`](std::convert::Infallible) when it always continues, which
    /// declares nothing.
    type Short: ShortCircuit;

    /// Handles a request, calling `next` to continue.
    ///
    /// `reads` arrives already extracted from the request headers; a failure to
    /// extract it is answered before this is called.
    fn intercept(
        &self,
        request: Request,
        reads: Self::Reads,
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Result<Continued<Self::Adds>, Self::Short>> + Send;
}

/// An interceptor configured with a combination it cannot honour.
///
/// Every other thing an interceptor declares is read from its types, so the
/// compiler catches it. These are the ones a *builder* decides at run time,
/// where there is no type to read — so they are checked once, while the router
/// is assembled, and never per request.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MiddlewareError {
    /// A [`Cors`](cors::Cors) permitted every origin and credentials together.
    ///
    /// The CORS protocol forbids `Access-Control-Allow-Origin: *` on a
    /// credentialed response, so there is no response satisfying both. Refusing
    /// is not pedantry: the fallback is to echo the request's own origin, which
    /// turns "allow any origin" plus "allow credentials" into
    /// reflect-any-origin-with-credentials — the most permissive configuration
    /// the protocol has, reached by asking for something else.
    #[error(
        "a CORS configuration permits any origin and also permits credentials, which the protocol \
         forbids; drop `allow_credentials`, or replace `allow_any_origin` with the origins \
         `allow_origins` should name"
    )]
    CredentialedWildcardOrigin,

    /// A [`Cors`](cors::Cors) exposed every response header and also permitted
    /// credentials.
    ///
    /// On a credentialed response the CORS protocol reads
    /// `Access-Control-Expose-Headers: *` as the literal field name `*` rather
    /// than as a wildcard, so the pair exposes nothing. Unlike the origin case
    /// the failure is silent — no browser reports it, and the headers are
    /// simply unreadable — which is what makes refusing it worth more than
    /// shipping it.
    #[error(
        "a CORS configuration exposes every response header and also permits credentials, which          the protocol reads as exposing a header literally named `*`; name the headers          `expose_headers` should expose, or drop `allow_credentials`"
    )]
    CredentialedWildcardExposure,
}

/// Merges `names` into whatever `Vary` a response already carries.
///
/// A union rather than an insert, because `Vary` is the one response header two
/// interceptors may both contribute to: RFC 9110 section 12.5.5 defines it as an
/// unordered set of field names, so `Compression` varying on `Accept-Encoding`
/// and `Cors` varying on `Origin` both belong on the same response. Overwriting
/// would leave a cache keying on one of the two, which is a stale-response bug
/// rather than a missing nicety.
///
/// Field names are case-insensitive (RFC 9110 section 5.1), so a name already
/// present in another spelling is not added again. `Vary: *` already says the
/// response depends on more than field names can express, so nothing narrows it.
pub(crate) fn vary_on(fields: &mut crate::http::HeaderMap, names: &'static [&'static str]) {
    if names.is_empty() {
        return;
    }

    let existing = fields
        .get(crate::http::header::VARY)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if existing.split(',').any(|name| name.trim() == "*") {
        return;
    }

    let mut merged: Vec<&str> = existing
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();

    for name in names {
        if !merged
            .iter()
            .any(|present| present.eq_ignore_ascii_case(name))
        {
            merged.push(name);
        }
    }

    // An unrepresentable value is dropped rather than panicking: every name
    // reaching this is a `&'static str` a `HeaderParams` implementation wrote
    // down, and a response path that panics is worse than one missing a cache
    // hint.
    if let Ok(value) = crate::http::HeaderValue::from_str(&merged.join(", ")) {
        fields.insert(crate::http::header::VARY, value);
    }
}

/// A response that came back through the rest of the chain.
///
/// Obtainable only from [`Next::run`], which is what makes
/// [`Interceptor::Short`] exhaustive: an interceptor either forwards what the
/// chain produced or answers with a type that describes itself, and there is no
/// third way to mint a response.
///
/// `H` records the headers attached so far. It starts as `()` and only
/// [`with_headers`](Continued::with_headers) changes it, so the headers an
/// interceptor declares and the headers it attaches are one fact.
#[must_use = "a `Continued` is the response; dropping it drops what the chain produced"]
pub struct Continued<H = ()> {
    response: Response,
    _headers: std::marker::PhantomData<fn() -> H>,
}

impl<H> std::fmt::Debug for Continued<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Continued")
            .field("status", &self.response.status())
            .finish_non_exhaustive()
    }
}

impl Continued<()> {
    /// Wraps what the rest of the chain produced.
    pub(crate) fn new(response: Response) -> Self {
        Self {
            response,
            _headers: std::marker::PhantomData,
        }
    }
}

impl<H: HeaderParams> Continued<H> {
    /// Attaches a declared header group.
    ///
    /// Changes the type, so an interceptor whose `Adds` names a group has to
    /// call this to return at all — and one whose `Adds` is `()` has nothing it
    /// could attach.
    pub fn with_headers<G: HeaderParams>(mut self, headers: G) -> Continued<G> {
        for (name, value) in headers.encode() {
            self.response.headers_mut().insert(name, value);
        }

        vary_on(self.response.headers_mut(), G::VARIES);

        Continued {
            response: self.response,
            _headers: std::marker::PhantomData,
        }
    }

    /// The status the chain produced.
    ///
    /// Readable because logging or metrics may want it; there is deliberately
    /// no way to *change* it, since a status an interceptor invents is a status
    /// no type declared.
    #[must_use]
    pub fn status(&self) -> crate::http::StatusCode {
        self.response.status()
    }

    /// The headers the chain produced.
    ///
    /// Readable, not writable: [`with_headers`](Continued::with_headers) is the
    /// only way to add one, and it is what keeps the added set equal to the
    /// declared set.
    #[must_use]
    pub fn headers(&self) -> &crate::http::HeaderMap {
        self.response.headers()
    }

    /// Takes the body out, leaving an empty one behind.
    ///
    /// Paired with [`set_body`](Continued::set_body) for anything that reads a
    /// response and hands the same bytes on. Two calls rather than one
    /// combinator because draining a body is asynchronous and fallible, and a
    /// closure returning a body can be neither.
    ///
    /// A body needs no declaration: it has no name to collide on, so two
    /// interceptors rewriting one compose where two setting one header do not.
    /// The status and the headers are untouched by both halves, which is what
    /// stops this becoming a way to mint a response.
    #[must_use = "the body is removed; put one back with `set_body`"]
    pub fn take_body(&mut self) -> crate::http::body::Body {
        std::mem::take(self.response.body_mut())
    }

    /// Puts a body back.
    ///
    /// What it does *not* license is changing what the body means. An encoding
    /// a consumer has to know about is a header, and a header has to be in
    /// [`Adds`](Interceptor::Adds) — which is why `Compression` declares
    /// `Content-Encoding` rather than quietly re-encoding behind this.
    pub fn set_body(&mut self, body: crate::http::body::Body) {
        *self.response.body_mut() = body;
    }

    /// Unwraps into the response, for the machinery that writes it.
    pub(crate) fn into_response(self) -> Response {
        self.response
    }
}

/// The remainder of the interceptor chain.
///
/// A cursor rather than a linked structure: running the rest of the chain is
/// taking the head of a slice, and reaching the end is calling the endpoint. A
/// route with no interceptors therefore pays nothing.
pub struct Next<'a, C> {
    remaining: &'a [Arc<dyn ErasedInterceptor<C>>],
    terminal: &'a dyn ErasedTerminal<C>,
    context: &'a C,
    route: Route<'a>,
}

impl<C> std::fmt::Debug for Next<'_, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Next")
            .field("remaining", &self.remaining.len())
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}

impl<'a, C: Sync + 'static> Next<'a, C> {
    /// Begins a chain.
    pub(crate) fn new(
        remaining: &'a [Arc<dyn ErasedInterceptor<C>>],
        terminal: &'a dyn ErasedTerminal<C>,
        context: &'a C,
        route: Route<'a>,
    ) -> Self {
        Self {
            remaining,
            terminal,
            context,
            route,
        }
    }

    /// Runs the rest of the chain.
    ///
    /// The only source of a [`Continued`], which is what leaves
    /// [`Interceptor::Short`] as the sole other way an interceptor can answer.
    pub async fn run(self, request: Request) -> Continued<()> {
        // Taking the head of the slice is the whole of "running the rest": an
        // empty remainder is the endpoint, so a route with no interceptors
        // reaches the handler with nothing in between.
        let response = match self.remaining.split_first() {
            Some((head, remaining)) => {
                let next = Self {
                    remaining,
                    terminal: self.terminal,
                    context: self.context,
                    route: self.route,
                };
                (**head).intercept(request, self.context, next).await
            }
            None => self.terminal.call(request, self.context).await,
        };

        Continued::new(response)
    }

    /// The operation this request matched.
    ///
    /// Always available: interceptors run per-operation, after routing.
    #[must_use]
    pub fn route(&self) -> Route<'a> {
        self.route
    }
}

/// Middleware that observes without altering.
///
/// Because it cannot change the exchange, it contributes nothing to the
/// description — which is why an observer needs no declaration and can see
/// everything, including the headers no extractor will surface to a handler.
///
/// `route` is `None` when no operation matched: a 404 is still worth logging,
/// and an observer that could not see one would be blind to exactly the
/// traffic worth investigating.
pub trait Observer<C>: Send + Sync + 'static {
    /// Called when a request arrives, before any interceptor.
    fn on_request(&self, request: &Request, route: Option<Route<'_>>, context: &C);

    /// Called when a response is about to be written.
    fn on_response(
        &self,
        response: &Response,
        route: Option<Route<'_>>,
        elapsed: std::time::Duration,
    );

    /// Called when a handler panicked.
    fn on_panic(&self, payload: &(dyn std::any::Any + Send), route: Option<Route<'_>>) {
        let _ = (payload, route);
    }
}

#[cfg(test)]
mod tests {
    use super::{Continued, HeaderParams};
    use crate::http::{HeaderName, HeaderValue, Response, header};

    /// A group that declares no header of its own and varies on `origin` —
    /// the shape `Cors` takes.
    struct VariesOnOrigin;

    impl HeaderParams for VariesOnOrigin {
        const NAMES: &'static [&'static str] = &[];
        const VARIES: &'static [&'static str] = &["origin"];

        fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
            Vec::new()
        }
    }

    /// The `Vary` a response carries after `headers` rides on it.
    fn vary_after<G: HeaderParams>(existing: Option<&str>, headers: G) -> Option<String> {
        let mut response = Response::new(crate::http::body::Body::empty());

        if let Some(existing) = existing {
            response.headers_mut().insert(
                header::VARY,
                HeaderValue::from_str(existing).expect("a representable Vary"),
            );
        }

        Continued::new(response)
            .with_headers(headers)
            .into_response()
            .headers()
            .get(header::VARY)
            .map(|value| value.to_str().expect("a printable Vary").to_owned())
    }

    /// The failure this exists to stop: `with_headers` used `insert`, so a
    /// second contribution replaced the first rather than joining it — and a
    /// response varying on two fields that advertised one is a cache poisoning
    /// bug rather than a missing nicety.
    #[test]
    fn a_vary_union_keeps_the_field_names_already_present() {
        let vary = vary_after(Some("accept"), VariesOnOrigin).expect("a Vary");
        let names: Vec<_> = vary.split(',').map(str::trim).collect();

        assert!(names.contains(&"accept"), "lost the existing field: {vary}");
        assert!(names.contains(&"origin"), "never added its own: {vary}");
    }

    /// `Vary` is a set of field names, and RFC 9110 section 5.1 makes a field
    /// name case-insensitive, so the same name in two spellings is one member.
    #[test]
    fn a_vary_union_adds_no_name_twice_whatever_its_case() {
        let vary = vary_after(Some("Origin"), VariesOnOrigin).expect("a Vary");
        let names: Vec<_> = vary.split(',').map(str::trim).collect();

        assert_eq!(names.len(), 1, "repeated one field name: {vary}");
    }

    /// `Vary: *` already says the response depends on more than the field names
    /// can express, so adding one narrows nothing and must not appear to.
    #[test]
    fn a_wildcard_vary_absorbs_every_name_added_to_it() {
        let vary = vary_after(Some("*"), VariesOnOrigin).expect("a Vary");

        assert_eq!(vary, "*");
    }

    /// A repeatable field reaches the wire once per value.
    ///
    /// `WithHeaders::into_response` appends for exactly this reason and says so:
    /// "a group naming `Set-Cookie` twice sends it twice instead of comma-joining
    /// two values that may not be joined". `Continued::with_headers` inserts,
    /// so the same group loses every value but the last — and
    /// `response/headers.rs` claims the two paths "cannot disagree".
    #[test]
    fn a_repeatable_group_reaches_the_wire_once_per_value() {
        struct TwoCookies;

        impl HeaderParams for TwoCookies {
            const NAMES: &'static [&'static str] = &["set-cookie"];
            const REPEATABLE: bool = true;

            fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
                vec![
                    (
                        header::SET_COOKIE,
                        HeaderValue::from_static("first=1; Path=/"),
                    ),
                    (
                        header::SET_COOKIE,
                        HeaderValue::from_static("second=2; Path=/"),
                    ),
                ]
            }
        }

        let sent: Vec<_> = Continued::new(Response::new(crate::http::body::Body::empty()))
            .with_headers(TwoCookies)
            .into_response()
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().expect("a printable field").to_owned())
            .collect();

        assert_eq!(sent, ["first=1; Path=/", "second=2; Path=/"]);
    }

    /// A group that is not repeatable replaces whatever was there.
    ///
    /// The control. Without it "repeatable appends" would read as "everything
    /// appends", and a second `Content-Encoding` beside a first is a response
    /// no client can decode.
    #[test]
    fn a_group_that_is_not_repeatable_replaces_the_value_already_set() {
        struct OneEncoding;

        impl HeaderParams for OneEncoding {
            const NAMES: &'static [&'static str] = &["content-encoding"];

            fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
                vec![(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"))]
            }
        }

        let mut response = Response::new(crate::http::body::Body::empty());
        response
            .headers_mut()
            .insert(header::CONTENT_ENCODING, HeaderValue::from_static("br"));

        let sent: Vec<_> = Continued::new(response)
            .with_headers(OneEncoding)
            .into_response()
            .headers()
            .get_all(header::CONTENT_ENCODING)
            .iter()
            .map(|value| value.to_str().expect("a printable field").to_owned())
            .collect();

        assert_eq!(sent, ["gzip"]);
    }

    /// A group varying on nothing leaves the header absent rather than empty.
    #[test]
    fn a_group_that_varies_on_nothing_writes_no_vary() {
        struct Silent;

        impl HeaderParams for Silent {
            const NAMES: &'static [&'static str] = &[];

            fn encode(&self) -> Vec<(HeaderName, HeaderValue)> {
                Vec::new()
            }
        }

        assert_eq!(vary_after(None, Silent), None);
    }
}
