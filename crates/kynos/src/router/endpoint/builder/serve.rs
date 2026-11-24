//! What a built endpoint does once mounted: describes its operation and serves
//! its requests, apart from the setters that assemble it.

use std::{borrow::Cow, future::Future, pin::Pin};

use kynos_openapi::{Method, PathTemplate};

use super::{EndpointBuilder, Named};
use crate::{
    handler::Handler,
    http::{Parts, Request, Response, body::Body},
    middleware::{
        Continued, MiddlewareError, Next, catch_panic::PanicPolicy, erased::ErasedTerminal,
    },
    router::{
        dispatch,
        endpoint::{self, Endpoint},
        install,
        operation::{OperationCx, OperationIdCase, Route},
    },
};

impl<C, H, A, P, I: 'static> Endpoint<C> for EndpointBuilder<C, H, A, P, I>
where
    C: Send + Sync + 'static,
    H: Handler<C, A>,
    // Only `'static`, as for `IntoEndpoints`.
    A: 'static,
    P: PanicPolicy,
{
    fn method(&self) -> Method {
        self.method
    }

    fn path(&self) -> &PathTemplate {
        &self.path
    }

    fn describe(&self, operation: &mut OperationCx<'_>) {
        // First, so the handler's responses win over an interceptor's.
        <H as Handler<C, A>>::describe(operation);

        for interceptor in &self.interceptors {
            interceptor.describe(operation);
        }

        match self.operation_id {
            Some(Named::Written(id)) => operation.set_operation_id(id),
            Some(Named::Derived(id)) => operation.set_derived_operation_id(id),
            None => {}
        }
        if let Some(summary) = self.summary {
            operation.set_summary(summary);
        }
        if let Some(description) = self.description {
            operation.set_description(description);
        }
        for tag in &self.tags {
            operation.add_tag(*tag);
        }
        operation.set_deprecated(self.deprecated);

        if install::catches::<P>() {
            dispatch::recovery::describe(operation);
        }
    }

    // Splits the request before the future exists and awaits each branch in
    // place: every matched request carries this future in a box, and the head
    // it owns is the only copy, which the handler borrows. A request argument,
    // or an `async fn` between the box and the handler, would keep another.
    fn call(&self, request: Request, context: &C) -> impl Future<Output = Response> + Send {
        let (mut parts, body) = request.into_parts();
        async move {
            if install::catches::<P>() {
                // Boxed by `recover`, so it costs this future a pointer.
                return match dispatch::recovery::recover(self.serve(parts, body, context)).await {
                    Ok(response) => response,
                    Err(payload) => dispatch::recovery::recovered_response(payload),
                };
            }

            // Without interceptors, no chain and no second box. `serve`'s
            // branches, inlined: awaiting `serve` would move the head into a
            // future of its own, beside the slot this one keeps for it.
            if self.interceptors.is_empty() {
                return Handler::call(self.handler.clone(), &mut parts, body, context).await;
            }

            self.chain(Request::from_parts(parts, body), context)
                .await
                .into_response()
        }
    }

    fn __unhonourable_interceptor(&self) -> Option<MiddlewareError> {
        self.interceptors
            .iter()
            .find_map(|interceptor| install::cors_conflict(interceptor))
    }

    fn __guarded(&self) -> bool {
        H::__GUARDED
    }

    fn __public(&self) -> bool {
        self.public
    }

    // The handler, or one of this operation's own interceptors, all of which
    // run beneath whatever covers it from outside.
    fn __reads_body(&self) -> bool {
        H::__READS_BODY
            || self
                .interceptors
                .iter()
                .any(|interceptor| interceptor.reads_body())
    }

    fn __described_head(&self, case: OperationIdCase) -> Option<Cow<'static, str>> {
        self.head.map(|head| head.cased(case))
    }

    // This operation's recovery covers its guard wherever the guard runs.
    fn __guard_ahead(
        &self,
        request: Request,
        context: &C,
    ) -> impl Future<Output = Result<Request, Response>> + Send {
        endpoint::guard_ahead::<C, H, A>(request, context, install::catches::<P>())
    }
}

impl<C, H, A, P, I: 'static> EndpointBuilder<C, H, A, P, I>
where
    C: Send + Sync + 'static,
    H: Handler<C, A>,
    // Only `'static`, as for `IntoEndpoints`.
    A: 'static,
    P: PanicPolicy,
{
    /// Runs the handler, through this operation's own interceptors where it
    /// has any; what recovery wraps, and so the owner of the head it lends.
    async fn serve(&self, mut parts: Parts, body: Body, context: &C) -> Response {
        if self.interceptors.is_empty() {
            Handler::call(self.handler.clone(), &mut parts, body, context).await
        } else {
            self.chain(Request::from_parts(parts, body), context)
                .await
                .into_response()
        }
    }

    /// Starts this operation's own interceptors around the handler, which is
    /// this builder itself as the chain's terminal.
    fn chain<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> impl Future<Output = Continued<()>> + Send + 'a {
        // The key routing matched, prefixes included, and the name the router
        // published, recased; this endpoint's own only when driven without a
        // router.
        let path = dispatch::mounted_path(&request).unwrap_or(self.path.as_str());
        let routed = dispatch::routed_operation_id(&request);
        // The `HEAD` this describes is the operation a HEAD request reaches.
        let route = match self.head {
            Some(head) if request.method() == crate::http::Method::HEAD => {
                Route::new(path, routed.unwrap_or(head.as_str()), Method::Head)
            }
            _ => Route::new(
                path,
                routed.unwrap_or(self.operation_id.map_or("", Named::as_str)),
                self.method,
            ),
        };
        Next::new(&self.interceptors, self, context, Some(route)).run(request)
    }
}

impl Named {
    /// The identifier as written or derived, before any case applies.
    fn as_str(self) -> &'static str {
        match self {
            Self::Written(id) | Self::Derived(id) => id,
        }
    }

    /// The identifier a router spelling derived ones in `case` publishes.
    fn cased(self, case: OperationIdCase) -> Cow<'static, str> {
        match self {
            Self::Written(id) => Cow::Borrowed(id),
            Self::Derived(id) => case.apply(id),
        }
    }
}

/// The handler, as the end of this endpoint's own interceptor chain: the
/// builder rather than a terminal built per request, so nothing the chain
/// borrows lives in the future driving it.
impl<C, H, A, P, I: 'static> ErasedTerminal<C> for EndpointBuilder<C, H, A, P, I>
where
    C: Send + Sync + 'static,
    H: Handler<C, A>,
    A: 'static,
    P: PanicPolicy,
{
    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        // The box owns the head the handler borrows.
        let (mut parts, body) = request.into_parts();
        Box::pin(
            async move { Handler::call(self.handler.clone(), &mut parts, body, context).await },
        )
    }
}
