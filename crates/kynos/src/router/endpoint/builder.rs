//! Building an endpoint at runtime rather than with a route attribute.

use kynos_openapi::{Method, PathTemplate};

use crate::{
    handler::Handler,
    http::{Request, Response},
    middleware::{
        Interceptor,
        catch_panic::{Catch, PanicPolicy, Propagate},
    },
    router::{
        endpoint::{
            Endpoint,
            set::{Endpoints, IntoEndpoints},
        },
        operation::{OperationCx, Tag},
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
#[derive(Debug)]
pub struct EndpointBuilder<C, H, A, P = Propagate> {
    // `fn() -> _` rather than the bare tuple: the parameters exist to name a
    // shape, and letting them decide whether this builder is `Send` would make
    // `Endpoints::push` reject handlers that are perfectly sound. The lint is
    // measuring the four parameters the type genuinely has.
    #[allow(clippy::type_complexity)]
    _private: std::marker::PhantomData<fn() -> (C, H, A, P)>,
}

impl<C, H: Handler<C, A>, A> EndpointBuilder<C, H, A, Propagate> {
    /// Begins an endpoint for `handler`.
    #[must_use]
    pub fn new(method: Method, path: PathTemplate, handler: H) -> Self {
        let _ = (method, path, handler);
        todo!()
    }
}

impl<C, H: Handler<C, A>, A, P: PanicPolicy> EndpointBuilder<C, H, A, P> {
    /// Begins an endpoint whose panic policy is already decided.
    ///
    /// What a route attribute expands into: the attribute knows the policy at
    /// compile time from `catch_panics` in its arguments, so it names it rather
    /// than starting at [`Propagate`] and transitioning.
    pub(crate) fn with_policy(method: Method, path: PathTemplate, handler: H) -> Self {
        let _ = (method, path, handler);
        todo!()
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
        todo!()
    }

    /// Sets the operation identifier.
    ///
    /// A route attribute defaults it to the handler's own name. Override it
    /// only to keep a generated client's method name stable across a refactor.
    #[must_use]
    pub fn operation_id(self, id: &'static str) -> Self {
        let _ = id;
        todo!()
    }

    /// Sets the operation summary.
    #[must_use]
    pub fn summary(self, summary: &'static str) -> Self {
        let _ = summary;
        todo!()
    }

    /// Sets the operation description.
    #[must_use]
    pub fn description(self, description: &'static str) -> Self {
        let _ = description;
        todo!()
    }

    /// Tags the operation.
    #[must_use]
    pub fn tag<T: Tag>(self) -> Self {
        todo!()
    }

    /// Marks the operation deprecated.
    #[must_use]
    pub fn deprecated(self) -> Self {
        todo!()
    }

    /// Applies an interceptor to this operation only.
    ///
    /// The interceptor's contribution is merged into this endpoint's
    /// description. Interceptors accumulate in a list rather than in the
    /// builder's type, so that a router, a group and an endpoint compose them
    /// the same way — and so that `routes![a, b]` still typechecks when only
    /// one of them carries an interceptor.
    #[must_use]
    pub fn intercept<N: Interceptor<C>>(self, interceptor: N) -> Self
    where
        C: Sync + 'static,
    {
        let _ = interceptor;
        todo!()
    }
}

impl<C, H, A, P> IntoEndpoints<C> for EndpointBuilder<C, H, A, P>
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
    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        sink.push(self);
    }
}

impl<C, H, A, P> Endpoint<C> for EndpointBuilder<C, H, A, P>
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
        todo!()
    }

    fn path(&self) -> &PathTemplate {
        todo!()
    }

    fn describe(&self, operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }

    async fn call(&self, request: Request, context: &C) -> Response {
        let _ = (request, context);
        todo!()
    }
}
