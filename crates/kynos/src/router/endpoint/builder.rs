//! Building an endpoint at runtime rather than with a route attribute.

use std::{future::Future, marker::PhantomData, pin::Pin, sync::Arc};

use kynos_openapi::{Method, PathTemplate};

use crate::{
    handler::Handler,
    http::{Request, Response},
    middleware::{
        Interceptor, Next,
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
        operation::{OperationCx, Route, Tag},
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
/// .intercept(kynos::middleware::limits::BodySize::new(1_024));
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
    tags: Vec<&'static str>,
    deprecated: bool,
    interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,

    // `fn() -> _` rather than the bare tuple: the parameters exist to name a
    // shape, and letting them decide whether this builder is `Send` would make
    // `Endpoints::push` reject handlers that are perfectly sound. The lint is
    // measuring the parameters the type genuinely has.
    //
    // `I` is the interceptors mounted on this one operation, which `mount`
    // checks against the router's own -- the reason `routes!` expands to a
    // tuple rather than to an already-erased collection.
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
    /// Begins an endpoint whose panic policy is already decided.
    ///
    /// What a route attribute expands into: the attribute knows the policy at
    /// compile time from `catch_panics` in its arguments, so it names it rather
    /// than starting at [`Propagate`] and transitioning.
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
    /// recovery branch.
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
    pub fn catch_panics(self) -> EndpointBuilder<C, H, A, Catch> {
        const {
            assert!(
                cfg!(panic = "unwind"),
                "Kynos panic recovery requires `panic = \"unwind\"`; remove `catch_panics` or enable unwinding"
            );
        }
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
    #[must_use]
    pub fn tag<T: Tag>(mut self) -> Self {
        self.tags.push(T::NAME);
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
    /// description. Interceptors accumulate in a list rather than in the
    /// builder's type, so that a router, a group and an endpoint compose them
    /// the same way — and so that `routes![a, b]` still typechecks when only
    /// one of them carries an interceptor.
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
    // `'static` and no more. `A` names the argument shape and never exists as
    // a value, so requiring it to be `Send + Sync` refuses handlers whose
    // *arguments* are not — a property of values the handler's own future
    // holds, never of this builder, which keeps `A` behind `PhantomData<fn()
    // -> _>` precisely so its auto traits do not leak.
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
    // `'static` and no more. `A` names the argument shape and never exists as
    // a value, so requiring it to be `Send + Sync` refuses handlers whose
    // *arguments* are not — a property of values the handler's own future
    // holds, never of this builder, which keeps `A` behind `PhantomData<fn()
    // -> _>` precisely so its auto traits do not leak.
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
        // The handler goes first, so that what it says about a status wins over
        // what an interceptor covering it contributes for the same one.
        <H as Handler<C, A>>::describe(operation);

        let route = Route::new(
            self.path.as_str(),
            self.operation_id.unwrap_or_default(),
            self.method,
        );
        for interceptor in &self.interceptors {
            interceptor.describe(route, operation);
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
            operation.add_tag(tag);
        }
        operation.set_deprecated(self.deprecated);

        if recovers::<P>() {
            let responses = dispatch::panic_responses(operation.registry());
            operation.add_responses(responses);
        }
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        // The terminal owns its handler because `ErasedTerminal` is `'static`,
        // and `Handler::call` consumes one regardless -- so the clone is the
        // same one a direct call would have made.
        let terminal = HandlerTerminal::<C, H, A> {
            handler: self.handler.clone(),
            _private: PhantomData,
        };

        // A route with no interceptors pays nothing: no chain is assembled and
        // the handler is entered directly.
        let served = async {
            if self.interceptors.is_empty() {
                ErasedTerminal::call(&terminal, request, context).await
            } else {
                let route = Route::new(
                    self.path.as_str(),
                    self.operation_id.unwrap_or_default(),
                    self.method,
                );
                Next::new(&self.interceptors, &terminal, context, route)
                    .run(request)
                    .await
                    .into_response()
            }
        };

        if recovers::<P>() {
            match dispatch::recover(served).await {
                Ok(response) => response,
                Err(_) => dispatch::panic_response(),
            }
        } else {
            served.await
        }
    }
}

/// The handler, as the end of this endpoint's own interceptor chain.
struct HandlerTerminal<C, H, A> {
    handler: H,
    _private: PhantomData<fn() -> (C, A)>,
}

impl<C, H, A> ErasedTerminal<C> for HandlerTerminal<C, H, A>
where
    C: Send + Sync + 'static,
    H: Handler<C, A>,
    A: 'static,
{
    fn describe(&self, operation: &mut OperationCx<'_>) {
        <H as Handler<C, A>>::describe(operation);
    }

    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(Handler::call(self.handler.clone(), request, context))
    }
}

/// Whether `P` selected recovery.
///
/// [`PanicPolicy`] is a marker with no members to read, so the policy is
/// resolved by identity -- which is exactly as static as the type it comes
/// from.
fn recovers<P: PanicPolicy>() -> bool {
    std::any::TypeId::of::<P>() == std::any::TypeId::of::<Catch>()
}
