//! Object-safe forms of the traits that use return-position `impl Trait`.
//!
//! Both box their future; staying `pub(crate)` keeps the box out of user
//! signatures.

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
    fn describe(&self, route: Route<'_>, operation: &mut OperationCx<'_>);

    fn intercept<'a>(
        &'a self,
        request: Request,
        context: &'a C,
        next: Next<'a, C>,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;

    /// This interceptor as a concrete value.
    ///
    /// The one exception to reading an interceptor from its types: the router
    /// downcasts to [`Cors`](crate::middleware::cors::Cors) alone. See
    /// `docs/middleware.md`, "The one interceptor the router recognises by
    /// identity".
    fn as_any(&self) -> &dyn std::any::Any;
}

impl<C, I> ErasedInterceptor<C> for I
where
    C: Sync + 'static,
    I: Interceptor<C>,
{
    fn describe(&self, route: Route<'_>, operation: &mut OperationCx<'_>) {
        // An interceptor declares the same thing for every operation it covers.
        let _ = route;

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
                // A `$ref` is already reachable through `components`.
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
            let reads = match <I::Reads as DecodeHeaders>::decode(request.headers()) {
                Ok(reads) => reads,
                Err(rejection) => return rejection.into_response(),
            };

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
/// Serving only: the description is assembled from the mounted endpoints, not
/// from terminals.
pub(crate) trait ErasedTerminal<C>: Send + Sync + 'static {
    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;
}
