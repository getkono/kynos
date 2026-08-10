//! Object-safe forms of the traits that use return-position `impl Trait`.
//!
//! A router holds interceptors and endpoints it cannot name individually, so
//! something has to erase them. Both traits here box their future, which is the
//! price of that; keeping them `pub(crate)` is what stops the price appearing
//! in a signature a user reads.
//!
//! `router::endpoint::DynEndpoint` is the same idea for endpoints, and
//! `router::service::ErasedService` for the whole service — three shims,
//! because RPITIT is worth having in all three public traits.

use std::{future::Future, pin::Pin};

use crate::{
    http::{Request, Response},
    middleware::{Interceptor, Next, contribution::OperationContribution},
    router::operation::{OperationCx, Route},
};

/// The object-safe form of [`Interceptor`].
// Driven by `Next::run` and `Router::build`, both still `todo!()`.
#[allow(dead_code)]
pub(crate) trait ErasedInterceptor<C>: Send + Sync + 'static {
    fn contribution(&self, route: Route<'_>) -> OperationContribution;

    fn intercept<'a>(
        &'a self,
        request: Request,
        context: &'a C,
        next: Next<'a, C>,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;
}

impl<C, I> ErasedInterceptor<C> for I
where
    C: Sync + 'static,
    I: Interceptor<C>,
{
    fn contribution(&self, route: Route<'_>) -> OperationContribution {
        Interceptor::contribution(self, route)
    }

    fn intercept<'a>(
        &'a self,
        request: Request,
        context: &'a C,
        next: Next<'a, C>,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(Interceptor::intercept(self, request, context, next))
    }
}

/// The end of a chain: whatever runs when no interceptor is left.
///
/// Separate from [`ErasedInterceptor`] because the terminal has no `next` to
/// call, and folding it in would mean every interceptor could be handed a chain
/// that ends in nothing.
#[allow(dead_code)]
pub(crate) trait ErasedTerminal<C>: Send + Sync + 'static {
    fn describe(&self, operation: &mut OperationCx<'_>);

    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;
}
