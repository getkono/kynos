//! Middleware that declares what it does to the contract.
//!
//! A `tower::Layer` can change the status, rewrite the body, add headers, or
//! refuse the request, and its type says nothing about which. Kynos splits
//! middleware in two instead:
//!
//! - An [`Interceptor`] can affect the exchange, and declares how in its own
//!   signature: the responses it can answer with, the headers it adds, and the
//!   headers it reads are three associated types. Attaching one to a group
//!   documents its effect on every operation underneath.
//! - An [`Observer`] sees everything and changes nothing, so it needs to
//!   declare nothing. Logging, tracing and metrics live here.
//!
//! The `unchecked` feature restores `Layer` support, at the price of a
//! description marked non-authoritative.
//!
//! # Wire-visible but contract-neutral
//!
//! Headers HTTP itself defines -- `Vary`, `Content-Encoding`, the CORS set --
//! are still *declared*, but their group sets [`HeaderParams::DESCRIBED`] to
//! `false`, so they stay out of the emitted description.
//!
//! [`HeaderParams::DESCRIBED`]: crate::extract::params::header::HeaderParams::DESCRIBED
//!
//! # The order a chain runs in
//!
//! **The first `intercept` call is the outermost interceptor.** Each scope's
//! own interceptors come before the ones a group or a nested router
//! contributed -- so a router's are outside a group's, and an endpoint's are
//! innermost of all.
//!
//! Order is not part of the type: [`CompatibleWith`](stack::CompatibleWith)
//! checks only that two interceptors do not add one header or answer with one
//! status. `docs/middleware.md` lists the arrangements that are wrong, such as
//! `Conditional` outside `Cache`.

pub mod catch_panic;
pub mod contribution;
pub mod cors;
pub mod csrf;
pub mod limits;
pub mod rate_limit;
pub mod request_id;
pub mod security_headers;
pub mod stack;

// Never `pub`, so `Pin<Box<dyn Future>>` reaches no user signature.
pub(crate) mod erased;

#[cfg(feature = "cache")]
pub mod cache;
#[cfg(feature = "compression")]
pub mod compression;
#[cfg(feature = "cache")]
pub mod conditional;
#[cfg(feature = "cookie")]
pub mod cookies;
#[cfg(feature = "compression")]
pub mod decompression;
#[cfg(feature = "trace")]
pub mod trace;

use std::{future::Future, sync::Arc};

use crate::{
    extract::params::header::{DecodeHeaders, EncodeHeaders},
    http::{Request, Response},
    middleware::erased::{ErasedInterceptor, ErasedTerminal},
    response::ShortCircuit,
    router::operation::Route,
};

/// Middleware that can affect the exchange, and says how in its own signature.
///
/// Each associated type is both the obligation and the declaration:
///
/// * [`Short`](Interceptor::Short) is the only way to answer without reaching
///   the handler, and its [`Responses`](crate::response::Responses) is what
///   the document prints. Use [`Infallible`](std::convert::Infallible) to
///   always continue.
/// * [`Adds`](Interceptor::Adds) is the response headers this interceptor
///   attaches. [`Next::run`] yields `Continued<()>` and
///   [`Continued::with_headers`] is the only way to reach `Continued<H>`, so
///   the headers attached are exactly the ones declared.
/// * [`Reads`](Interceptor::Reads) is the request headers it consumes, handed
///   over already extracted.
///
/// The `C: Sync + 'static` bound is what makes [`Next`] `Send`
/// unconditionally.
///
/// # What is left undeclared
///
/// [`Continued::take_body`] and [`Continued::set_body`] rewrite a body without
/// declaring anything. Injecting a route and retrying are not expressible here
/// at all; the first is what the `unchecked` escape hatches are for. See
/// [`docs/middleware.md`] for the invariant this buys and the one it does not.
///
/// [`docs/middleware.md`]: https://github.com/getkono/kynos/blob/master/docs/middleware.md
pub trait Interceptor<C: Sync + 'static>: Send + Sync + 'static {
    /// Request headers this interceptor reads, and therefore declares.
    ///
    /// `()` when it reads none.
    type Reads: DecodeHeaders + Send;

    /// Response headers this interceptor adds to a forwarded response.
    ///
    /// `()` when it adds none.
    type Adds: EncodeHeaders;

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
/// Checked once, while the router is assembled, and never per request.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MiddlewareError {
    /// A [`Cors`](cors::Cors) permitted every origin and credentials together.
    ///
    /// The CORS protocol forbids `Access-Control-Allow-Origin: *` on a
    /// credentialed response, and echoing the request's origin instead would
    /// grant every origin credentialed access.
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
    /// than as a wildcard, so the pair silently exposes nothing.
    #[error(
        "a CORS configuration exposes every response header and also permits credentials, which \
         the protocol reads as exposing a header literally named `*`; name the headers \
         `expose_headers` should expose, or drop `allow_credentials`"
    )]
    CredentialedWildcardExposure,
}

/// Merges `names` into whatever `Vary` a response already carries.
///
/// A union, since `Vary` is a set several interceptors contribute to (RFC 9110
/// section 12.5.5). Names compare case-insensitively (section 5.1), every
/// `Vary` line counts (section 5.3), and `Vary: *` is never narrowed. Merged
/// over bytes so a non-UTF-8 line survives; lines are rewritten only when a
/// name is added.
pub(crate) fn vary_on(fields: &mut crate::http::HeaderMap, names: &'static [&'static str]) {
    if names.is_empty() {
        return;
    }

    let mut merged: Vec<&[u8]> = Vec::new();

    for line in fields.get_all(crate::http::header::VARY) {
        for name in line.as_bytes().split(|&byte| byte == b',') {
            let name = name.trim_ascii();

            if name == b"*" {
                return;
            }

            if !name.is_empty() {
                merged.push(name);
            }
        }
    }

    let present = merged.len();

    for name in names {
        if !merged
            .iter()
            .any(|present| present.eq_ignore_ascii_case(name.as_bytes()))
        {
            merged.push(name.as_bytes());
        }
    }

    if merged.len() == present {
        return;
    }

    // Unreachable in practice; a missing cache hint beats a panicking response.
    if let Ok(value) = crate::http::HeaderValue::from_bytes(&merged.join(&b", "[..])) {
        fields.insert(crate::http::header::VARY, value);
    }
}

/// A response that came back through the rest of the chain.
///
/// Obtainable only from [`Next::run`], so an interceptor either forwards what
/// the chain produced or answers with its [`Interceptor::Short`].
///
/// `H` records the header group attached. It starts as `()` and only
/// [`with_headers`](Continued::with_headers) changes it.
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

    /// Attaches a declared header group.
    ///
    /// Changes the type, so an interceptor whose `Adds` names a group has to
    /// call this to return at all. Available on `Continued<()>` alone, so a
    /// response carries exactly one group.
    // By value so the call reads `.with_headers(Group { .. })`.
    #[allow(clippy::needless_pass_by_value)]
    pub fn with_headers<G: EncodeHeaders>(mut self, headers: G) -> Continued<G> {
        crate::extract::params::header::write(self.response.headers_mut(), &headers);

        Continued {
            response: self.response,
            _headers: std::marker::PhantomData,
        }
    }
}

// Construction, not a bound, keeps `H` a header group.
impl<H> Continued<H> {
    /// The status the chain produced.
    ///
    /// Read-only: a status an interceptor invents is one no type declared.
    #[must_use]
    pub fn status(&self) -> crate::http::StatusCode {
        self.response.status()
    }

    /// The headers the chain produced.
    ///
    /// Read-only: [`with_headers`](Continued::with_headers) is the only way to
    /// add one.
    #[must_use]
    pub fn headers(&self) -> &crate::http::HeaderMap {
        self.response.headers()
    }

    /// The extensions the chain produced.
    ///
    /// Read-only. A handler puts a value in and an interceptor that knows the
    /// type takes it out; an extension has no wire form, so nothing is
    /// described.
    #[must_use]
    pub fn extensions(&self) -> &crate::http::Extensions {
        self.response.extensions()
    }

    /// Takes the body out, leaving an empty one behind.
    ///
    /// Paired with [`set_body`](Continued::set_body) for anything that reads a
    /// response and hands the same bytes on. The status and headers are
    /// untouched by both halves.
    #[must_use = "the body is removed; put one back with `set_body`"]
    pub fn take_body(&mut self) -> crate::http::body::Body {
        std::mem::take(self.response.body_mut())
    }

    /// Puts a body back.
    ///
    /// An encoding a consumer has to know about is a header, so it still
    /// belongs in [`Adds`](Interceptor::Adds).
    pub fn set_body(&mut self, body: crate::http::body::Body) {
        *self.response.body_mut() = body;
    }

    /// Removes a field the declared group `G` names.
    ///
    /// For a field the interceptor owns that has stopped being true, such as
    /// `Content-Length` over a streamed encode, or `Strict-Transport-Security`
    /// over plain transport.
    pub(crate) fn remove_declared<G: crate::extract::params::header::HeaderParams>(
        &mut self,
        name: &crate::http::HeaderName,
    ) {
        debug_assert!(
            G::NAMES
                .iter()
                .any(|declared| stack::header_name_eq(declared, name.as_str())),
            "`{}` removes `{}`, which its `NAMES` does not declare",
            std::any::type_name::<G>(),
            name.as_str(),
        );

        self.response.headers_mut().remove(name);
    }

    /// Unwraps into the response, for the machinery that writes it.
    pub(crate) fn into_response(self) -> Response {
        self.response
    }
}

/// The remainder of the interceptor chain.
///
/// A cursor over a slice; reaching the end calls the endpoint.
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
    /// The only source of a [`Continued`].
    pub async fn run(self, request: Request) -> Continued<()> {
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
/// It contributes nothing to the description, so it needs no declaration and
/// can see everything.
///
/// `route` is `None` when no operation matched.
pub trait Observer<C>: Send + Sync + 'static {
    /// Called when a request arrives, before any interceptor.
    fn on_request(&self, request: &Request, route: Option<Route<'_>>, context: &C);

    /// Called when a response is about to be written.
    ///
    /// `elapsed` measures producing the response head, not delivering the
    /// body; a streaming response is reported long before its last frame.
    /// [`on_disconnect`](Observer::on_disconnect) reports a body that never
    /// finished.
    fn on_response(
        &self,
        response: &Response,
        route: Option<Route<'_>>,
        elapsed: std::time::Duration,
    );

    /// Called when a response body was dropped before its end.
    ///
    /// The peer did not receive the response
    /// [`on_response`](Observer::on_response) already reported: a download
    /// cancelled, a long poll abandoned, an event stream whose reader went
    /// away. A body that failed part-way is reported the same way.
    ///
    /// `elapsed` runs from the request arriving to the body being dropped.
    ///
    /// A client that leaves while the handler is still working is not
    /// reported, since there is no response body to drop yet.
    ///
    /// Called once the body has been released, so whatever it owned is gone;
    /// a body whose own drop panics is still reported, once. Called from the
    /// drop on whatever task last held the body: record and return, never
    /// block or await.
    fn on_disconnect(&self, route: Option<Route<'_>>, elapsed: std::time::Duration) {
        let _ = (route, elapsed);
    }

    /// Called when a panic was recovered, at whichever scope asked for
    /// recovery — the router, a group or one endpoint — before
    /// [`on_response`](Observer::on_response) sees the 500 it became.
    ///
    /// `route` is always `Some`, naming the operation the panic unwound out
    /// of. A panic is reported once, by the innermost scope that recovered it.
    /// Not reported: a panic nothing recovers, or one whose 500 an outer
    /// interceptor replaced with a short circuit of its own.
    fn on_panic(&self, payload: &(dyn std::any::Any + Send), route: Option<Route<'_>>) {
        let _ = (payload, route);
    }
}

#[cfg(test)]
mod tests;
