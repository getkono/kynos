//! Object-safe forms of the traits that use return-position `impl Trait`.
//!
//! Both box their future; staying `pub(crate)` keeps the box out of user
//! signatures.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use kynos_openapi::RefOr;

use crate::{
    error::{
        members::{Head, ProblemOrigin, served},
        problem::IntoProblem,
        rejection::{HeaderRejection, constraint_responses},
    },
    extract::params::header::{DecodeHeaders, HeaderParams},
    http::{Request, Response},
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses},
    router::operation::OperationCx,
};

/// The object-safe form of [`Interceptor`].
pub(crate) trait ErasedInterceptor<C>: Send + Sync + 'static {
    /// Adds this interceptor's three declarations to `operation`, which are
    /// the same for every operation it covers.
    fn describe(&self, operation: &mut OperationCx<'_>);

    /// Hands `request` to this interceptor, boxing the future it returns and
    /// nothing else.
    ///
    /// Decodes [`Interceptor::Reads`] at the call rather than in the box, so
    /// the box does not also keep the arguments that future already holds.
    fn intercept<'a>(
        &'a self,
        request: Request,
        context: &'a C,
        next: Next<'a, C>,
    ) -> Intercepted<'a>;

    /// This interceptor as a concrete value.
    ///
    /// The one exception to reading an interceptor from its types: the router
    /// downcasts to [`Cors`](crate::middleware::cors::Cors) alone. See
    /// `docs/middleware.md`, "The one interceptor the router recognises by
    /// identity".
    fn as_any(&self) -> &dyn std::any::Any;
}

/// An interceptor's own future, boxed as it is and polled for the response it
/// settles to.
///
/// Erased as [`Settle`] rather than as `Future<Output = Response>`: the latter
/// needs a wrapper mapping the outcome, and an `async` block awaiting a future
/// it captured keeps that future twice.
pub(crate) type Intercepted<'a> = Pin<Box<dyn Settle + Send + 'a>>;

/// A future whose outcome becomes a response once it is ready.
pub(crate) trait Settle {
    fn poll_settle(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Response>;
}

impl<F, A, S> Settle for F
where
    F: Future<Output = Result<Continued<A>, S>>,
    S: IntoResponse,
{
    fn poll_settle(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Response> {
        self.poll(context).map(|outcome| match outcome {
            Ok(continued) => Continued::into_response(continued),
            Err(short) => IntoResponse::into_response(short),
        })
    }
}

impl<C, I> ErasedInterceptor<C> for I
where
    C: Sync + 'static,
    I: Interceptor<C>,
{
    fn describe(&self, operation: &mut OperationCx<'_>) {
        if <I::Reads as HeaderParams>::DESCRIBED {
            let parameters = <I::Reads as HeaderParams>::parameters(operation.registry());
            for parameter in parameters {
                operation.add_parameter(parameter);
            }
        }

        // Whether or not the group is described: an undescribed one still
        // answers the 400, as a handler's `Headers` extractor does, and with
        // the members the router installs, as `intercept` renders it.
        if <I::Reads as DecodeHeaders>::FALLIBLE {
            let rejected = <HeaderRejection as Responses>::__served_responses(operation.registry());
            operation.add_responses(&rejected);
            let named = constraint_responses(
                operation.registry(),
                crate::http::StatusCode::BAD_REQUEST,
                &<I::Reads as HeaderParams>::constraint_types(),
                true,
            );
            operation.add_responses(&named);
        }

        let responses = <I::Short as Responses>::responses(operation.registry());
        operation.add_responses(&responses);

        if <I::Adds as HeaderParams>::DESCRIBED {
            let headers = <I::Adds as HeaderParams>::response_headers(operation.registry());
            for (name, header) in headers {
                // A `$ref` is already reachable through `components`.
                if let RefOr::Item(header) = header {
                    operation.place_response_header(I::ADDS_ON, name, &header);
                }
            }
        }
    }

    fn intercept<'a>(
        &'a self,
        request: Request,
        context: &'a C,
        next: Next<'a, C>,
    ) -> Intercepted<'a> {
        let reads = match <I::Reads as DecodeHeaders>::decode(request.headers()) {
            Ok(reads) => reads,
            Err(rejection) => {
                debug_assert!(
                    <I::Reads as DecodeHeaders>::FALLIBLE,
                    "`{}` sets `FALLIBLE = false` and rejected a request",
                    std::any::type_name::<I::Reads>(),
                );
                return refuse(rejection, request);
            }
        };

        Box::pin(Interceptor::intercept(self, request, reads, context, next))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Renders an interceptor's header rejection while the request is in hand,
/// so it carries the members the router installs.
///
/// Takes the request by value: a borrow of it in `intercept` changes the
/// codegen of the path that moves it, even where this arm is dead, at a cost
/// paid on every layer of every request.
#[cold]
#[inline(never)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "moving the request in is the point; see above"
)]
fn refuse<'a>(rejection: HeaderRejection, request: Request) -> Intercepted<'a> {
    let refused = served(
        IntoProblem::into_problem(rejection),
        ProblemOrigin::HeaderField,
        Head::from(&request),
    );
    Box::pin(std::future::ready(Err::<Continued, _>(refused)))
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
