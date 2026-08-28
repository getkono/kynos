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

use kynos_openapi::{RefOr, StatusPattern};

use crate::{
    extract::params::header::{DecodeHeaders, HeaderParams},
    http::{Request, Response},
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses},
    router::operation::{OperationCx, Route},
};

/// The object-safe form of [`Interceptor`].
pub(crate) trait ErasedInterceptor<C>: Send + Sync + 'static {
    /// Adds this interceptor's three declarations to `operation`.
    ///
    /// The whole of what an interceptor says, read from its associated types
    /// rather than from a value it returned -- which is why there is nothing
    /// here for an implementation to get wrong.
    fn describe(&self, route: Route<'_>, operation: &mut OperationCx<'_>);

    fn intercept<'a>(
        &'a self,
        request: Request,
        context: &'a C,
        next: Next<'a, C>,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;

    /// This interceptor as a concrete value.
    ///
    /// Everything a router reads about an interceptor is read from its *types*,
    /// which is the property `docs/middleware.md` opens with: a declaration
    /// cannot disagree with behaviour because it is the same text. This is the
    /// one deliberate exception, and it is bounded on both sides.
    ///
    /// It exists because two questions are about a *configuration* rather than
    /// a type: whether a [`Cors`](crate::middleware::cors::Cors) was built with
    /// a combination it cannot honour, and what a preflight on the paths it
    /// covers should answer. Neither is expressible as an associated type — a
    /// builder decides both at run time — and neither reaches the description.
    ///
    /// What keeps this from becoming a general capability: nothing dispatches
    /// on it. The router downcasts to the one type it knows about, that type's
    /// state parameter is sealed so the set of instantiations is closed, and a
    /// third-party interceptor is simply never asked. There is no trait method
    /// an outside implementation could supply to be read this way.
    fn as_any(&self) -> &dyn std::any::Any;
}

impl<C, I> ErasedInterceptor<C> for I
where
    C: Sync + 'static,
    I: Interceptor<C>,
{
    fn describe(&self, route: Route<'_>, operation: &mut OperationCx<'_>) {
        // An interceptor declares the same thing for every operation it covers.
        // Declaring differently per operation is expressed by mounting a
        // different instance at a different scope, which is why nothing here
        // reads the route.
        let _ = route;

        // The whole of what an interceptor says, read from its associated types
        // rather than from a value it returned. A group that is declared but not
        // described contributes nothing here and still collides at compile time.
        if <I::Reads as HeaderParams>::DESCRIBED {
            let parameters = <I::Reads as HeaderParams>::parameters(operation.registry());
            for parameter in parameters {
                operation.add_parameter(parameter);
            }
        }

        let responses = <I::Short as Responses>::responses(operation.registry());
        operation.add_responses(&responses);

        if <I::Adds as HeaderParams>::DESCRIBED {
            let headers = <I::Adds as HeaderParams>::response_headers(operation.registry());
            for (name, header) in headers {
                // A `$ref` names a header defined under `components`, which the
                // operation already reaches; only an inline definition has
                // anything to add here.
                if let RefOr::Item(header) = header {
                    operation.add_response_header(StatusPattern::Success, name, &header);
                }
            }
        }
    }

    fn intercept<'a>(
        &'a self,
        request: Request,
        context: &'a C,
        next: Next<'a, C>,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(async move {
            // Extraction happens here so that `intercept` receives what it
            // declared rather than a request to go looking in.
            let reads = match <I::Reads as DecodeHeaders>::decode(request.headers()) {
                Ok(reads) => reads,
                Err(rejection) => return rejection.into_response(),
            };

            // The two ways to answer, collapsed into the one the writer needs.
            match Interceptor::intercept(self, request, reads, context, next).await {
                Ok(continued) => Continued::into_response(continued),
                Err(short) => IntoResponse::into_response(short),
            }
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The end of a chain: whatever runs when no interceptor is left.
///
/// Separate from [`ErasedInterceptor`] because the terminal has no `next` to
/// call, and folding it in would mean every interceptor could be handed a chain
/// that ends in nothing.
///
/// Serving only. A terminal has no `describe`, because the description is
/// assembled from the *endpoints* a router mounted rather than from the
/// terminals it later builds out of them — so a `describe` here would be a
/// second answer to a question already answered, reachable from nothing.
pub(crate) trait ErasedTerminal<C>: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;
}
