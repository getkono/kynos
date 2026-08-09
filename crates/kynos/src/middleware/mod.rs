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

#[cfg(feature = "compression")]
pub mod compression;
#[cfg(feature = "trace")]
pub mod trace;

use std::future::Future;

use crate::{
    http::{Request, Response},
    middleware::contribution::OperationContribution,
};

/// Middleware that can affect the exchange, and says how.
pub trait Interceptor<C>: Send + Sync + 'static {
    /// What this interceptor adds to every operation it covers.
    ///
    /// Called once per operation while the router is built, never per request.
    fn contribution(&self) -> OperationContribution;

    /// Handles a request, calling `next` to continue.
    fn intercept(
        &self,
        request: Request,
        context: &C,
        next: Next<'_, C>,
    ) -> impl Future<Output = Response> + Send;
}

/// The remainder of the interceptor chain.
#[derive(Debug)]
pub struct Next<'a, C> {
    _private: std::marker::PhantomData<&'a C>,
}

impl<C> Next<'_, C> {
    /// Runs the rest of the chain.
    pub async fn run(self, request: Request) -> Response {
        let _ = request;
        todo!()
    }
}

/// Middleware that observes without altering.
///
/// Because it cannot change the exchange, it contributes nothing to the
/// description — which is why an observer needs no declaration and can see
/// everything, including the headers no extractor will surface to a handler.
pub trait Observer<C>: Send + Sync + 'static {
    /// Called when a request arrives, before any interceptor.
    fn on_request(&self, request: &Request, context: &C);

    /// Called when a response is about to be written.
    fn on_response(&self, response: &Response, elapsed: std::time::Duration);

    /// Called when a handler panicked.
    fn on_panic(&self, payload: &(dyn std::any::Any + Send)) {
        let _ = payload;
    }
}
