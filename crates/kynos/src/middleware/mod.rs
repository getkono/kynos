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
//! - An [`Interceptor`] can affect the exchange, and must declare an
//!   [`OperationContribution`] saying how. Contributions merge into every
//!   operation the interceptor covers.
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
//! # Out-of-document middleware
//!
//! Some things are wire-visible but contract-neutral: no operation in `paths`
//! changes shape. CORS preflight, response compression, trailing-slash
//! normalization. These ship as ordinary configuration with no contribution,
//! because there is nothing to declare.
//!
//! # How this module is laid out
//!
//! The two traits and the contribution type live here; every interceptor Kynos
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
    http::{Request, Response},
    middleware::{
        contribution::OperationContribution,
        erased::{ErasedInterceptor, ErasedTerminal},
    },
    router::operation::Route,
};

/// Middleware that can affect the exchange, and says how.
///
/// The `C: Sync + 'static` bound is stated once here rather than repeated on
/// every implementation: it is what makes [`Next`] `Send` unconditionally, so
/// no interceptor has to reason about whether its own future is.
pub trait Interceptor<C: Sync + 'static>: Send + Sync + 'static {
    /// What this interceptor adds to the description of `route`.
    ///
    /// Called once per covered operation while the router is built, never per
    /// request — which is what makes the emitted description checkable in CI
    /// rather than only observable by running the service.
    ///
    /// `route` is supplied so that one interceptor may contribute differently
    /// to different operations: different scopes per resource, a different
    /// documented limit per operation. It is still inert data, because the
    /// operation is known at build time.
    fn contribution(&self, route: Route<'_>) -> OperationContribution;

    /// Handles a request, calling `next` to continue.
    fn intercept(
        &self,
        request: Request,
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Response> + Send;
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
    pub async fn run(self, request: Request) -> Response {
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
