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

// Object-safe forms of the two RPITIT traits, so a heterogeneous chain fits in
// one collection. Private: `Pin<Box<dyn Future>>` never reaches a user
// signature.
mod erased;

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
    // Called by `Next::run`, whose body is still `todo!()`.
    #[allow(dead_code)]
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
// Read by `Next::run` and populated by `Router::build`, both still `todo!()`.
#[allow(dead_code)]
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
    // Called by `Router::build`, whose body is still `todo!()`.
    #[allow(dead_code)]
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
        let _ = request;
        todo!()
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
