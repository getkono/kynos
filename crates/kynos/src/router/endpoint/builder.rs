//! Building an endpoint at runtime rather than with a route attribute.

use std::{future::Future, marker::PhantomData, pin::Pin, sync::Arc};

use kynos_openapi::{Method, PathTemplate};

use crate::{
    handler::Handler,
    http::{Parts, Request, Response, body::Body},
    middleware::{
        Continued, Interceptor, MiddlewareError, Next,
        catch_panic::{Catch, PanicPolicy, Propagate},
        erased::{ErasedInterceptor, ErasedTerminal},
        stack::{CompatibleWith, Cons},
    },
    router::{
        dispatch,
        endpoint::{
            Endpoint,
            set::{Endpoints, IntoEndpoints},
        },
        install,
        operation::{DeclaredTag, OperationCx, Route, Tag},
    },
};

/// Builds an endpoint at runtime, rather than with a route attribute.
///
/// The attribute macros expand into this. Reach for it directly only when the
/// set of routes is not known at compile time; the attribute checks things this
/// cannot, notably that a handler's path parameters match its path template.
///
/// ```no_run
/// # use kynos::{openapi, router::endpoint::builder::EndpointBuilder};
/// async fn health() -> kynos::response::status::NoContent {
///     kynos::response::status::NoContent
/// }
///
/// let endpoint = EndpointBuilder::new(
///     openapi::Method::Get,
///     openapi::PathTemplate::parse("/health").expect("valid path"),
///     health,
/// )
/// .intercept(kynos::middleware::limits::body_size::BodySize::new(1_024));
/// let router = kynos::Router::<()>::new().mount(endpoint);
/// # let _ = router;
/// ```
pub struct EndpointBuilder<C, H, A, P = Propagate, I = ()> {
    method: Method,
    path: PathTemplate,
    handler: H,
    operation_id: Option<&'static str>,
    summary: Option<&'static str>,
    description: Option<&'static str>,
    tags: Vec<DeclaredTag>,
    deprecated: bool,
    interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,

    // `fn() -> _` keeps the parameters out of this builder's auto traits. `I`
    // is this operation's interceptor list, which `mount` checks.
    #[allow(clippy::type_complexity)]
    _private: PhantomData<fn() -> (C, H, A, P, I)>,
}

impl<C, H, A, P, I> std::fmt::Debug for EndpointBuilder<C, H, A, P, I> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EndpointBuilder")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("operation_id", &self.operation_id)
            .field("interceptors", &self.interceptors.len())
            .finish_non_exhaustive()
    }
}

impl<C, H: Handler<C, A>, A> EndpointBuilder<C, H, A, Propagate, ()> {
    /// Begins an endpoint for `handler`.
    #[must_use]
    pub fn new(method: Method, path: PathTemplate, handler: H) -> Self {
        Self::with_policy(method, path, handler)
    }
}

impl<C, H: Handler<C, A>, A, P: PanicPolicy, I> EndpointBuilder<C, H, A, P, I> {
    /// Begins an endpoint whose panic policy is already decided, as a route
    /// attribute knows it.
    pub(crate) fn with_policy(method: Method, path: PathTemplate, handler: H) -> Self {
        Self {
            method,
            path,
            handler,
            operation_id: None,
            summary: None,
            description: None,
            tags: Vec::new(),
            deprecated: false,
            interceptors: Vec::new(),
            _private: PhantomData,
        }
    }

    /// Carries every field across a change of type parameter.
    fn retype<Q, J>(self) -> EndpointBuilder<C, H, A, Q, J> {
        EndpointBuilder {
            method: self.method,
            path: self.path,
            handler: self.handler,
            operation_id: self.operation_id,
            summary: self.summary,
            description: self.description,
            tags: self.tags,
            deprecated: self.deprecated,
            interceptors: self.interceptors,
            _private: PhantomData,
        }
    }

    /// Converts panics from this operation into a documented 500 response.
    ///
    /// Extraction and handler execution are covered. The policy is carried in
    /// the endpoint's type, so an endpoint that does not select it installs no
    /// recovery branch. A panic recovered here reaches the router's observers
    /// through [`Observer::on_panic`](crate::middleware::Observer::on_panic),
    /// as one recovered at router or group scope does.
    ///
    /// Only the panic policy changes: the result keeps the interceptor list
    /// `I`, so an interceptor added after this is still checked against the
    /// ones added before it.
    ///
    /// # Compile-time requirement
    ///
    /// The final binary must use `panic = "unwind"`. Selecting this policy in
    /// a `panic = "abort"` build is a compile-time error.
    ///
    /// ```no_run
    /// # use kynos::{openapi, router::endpoint::builder::EndpointBuilder};
    /// async fn health() -> kynos::response::status::NoContent {
    ///     kynos::response::status::NoContent
    /// }
    ///
    /// let endpoint = EndpointBuilder::<(), _, _>::new(
    ///     openapi::Method::Get,
    ///     openapi::PathTemplate::parse("/health").expect("valid path"),
    ///     health,
    /// )
    /// .catch_panics();
    /// # let _ = endpoint;
    /// ```
    #[must_use]
    pub fn catch_panics(self) -> EndpointBuilder<C, H, A, Catch, I> {
        const { install::require_unwind() }
        self.retype()
    }

    /// Sets the operation identifier.
    ///
    /// A route attribute defaults it to the handler's own name. Override it
    /// only to keep a generated client's method name stable across a refactor.
    #[must_use]
    pub fn operation_id(mut self, id: &'static str) -> Self {
        self.operation_id = Some(id);
        self
    }

    /// Sets the operation summary.
    #[must_use]
    pub fn summary(mut self, summary: &'static str) -> Self {
        self.summary = Some(summary);
        self
    }

    /// Sets the operation description.
    #[must_use]
    pub fn description(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }

    /// Tags the operation.
    ///
    /// The tag's metadata reaches the document's own `tags` with it, so an
    /// operation tagged only here is not filed under a heading nothing
    /// declares.
    #[must_use]
    pub fn tag<T: Tag>(mut self) -> Self {
        self.tags.push(DeclaredTag::of::<T>());
        self
    }

    /// Tags the operation with a tag already resolved to a value, as
    /// `from_meta` holds it.
    pub(crate) fn with_tag(mut self, tag: DeclaredTag) -> Self {
        self.tags.push(tag);
        self
    }

    /// Marks the operation deprecated.
    #[must_use]
    pub fn deprecated(mut self) -> Self {
        self.deprecated = true;
        self
    }

    /// Applies an interceptor to this operation only.
    ///
    /// The interceptor's contribution is merged into this endpoint's
    /// description.
    ///
    /// The first call is the outermost of this endpoint's own, and every one of
    /// them is innermost of all: a router's and a group's both wrap them. See
    /// [the module's ordering rule](crate::middleware#the-order-a-chain-runs-in).
    #[must_use]
    pub fn intercept<N: Interceptor<C>>(
        self,
        interceptor: N,
    ) -> EndpointBuilder<C, H, A, P, Cons<N, I>>
    where
        C: Sync + 'static,
        I: CompatibleWith<N, C>,
    {
        let () = <I as CompatibleWith<N, C>>::CHECK;

        let mut builder: EndpointBuilder<C, H, A, P, Cons<N, I>> = self.retype();
        builder.interceptors.push(Arc::new(interceptor));
        builder
    }
}

impl<C, H, A, P, I> IntoEndpoints<C> for EndpointBuilder<C, H, A, P, I>
where
    C: Send + Sync + 'static,
    H: Handler<C, A>,
    // Only `'static`: `A` is a phantom argument shape, never a value.
    A: 'static,
    P: PanicPolicy,
    I: 'static,
{
    /// Whatever `intercept` accumulated on this one operation.
    type Stacks = I;

    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        sink.push(self);
    }
}

impl<C, H, A, P, I: 'static> Endpoint<C> for EndpointBuilder<C, H, A, P, I>
where
    C: Send + Sync + 'static,
    H: Handler<C, A>,
    // Only `'static`, as above.
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

        if let Some(id) = self.operation_id {
            operation.set_operation_id(id);
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
            let responses = dispatch::recovery::panic_responses(operation.registry());
            operation.add_responses(&responses);
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
}

impl<C, H, A, P, I: 'static> EndpointBuilder<C, H, A, P, I>
where
    C: Send + Sync + 'static,
    H: Handler<C, A>,
    // Only `'static`, as above.
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
        // The key routing matched, prefixes included; this endpoint's own
        // template only when driven without a router.
        let path = dispatch::mounted_path(&request).unwrap_or(self.path.as_str());
        let route = Route::new(path, self.operation_id.unwrap_or_default(), self.method);
        Next::new(&self.interceptors, self, context, Some(route)).run(request)
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
