//! One declared operation: what a route attribute produces, and the builder it
//! expands into.

use std::{future::Future, pin::Pin, sync::Arc};

use kynos_openapi::{Method, PathTemplate};

use crate::{
    handler::Handler,
    http::{Request, Response},
    middleware::{
        Interceptor,
        catch_panic::{Catch, PanicPolicy, Propagate},
        contribution::OperationContribution,
    },
    router::operation::{OperationCx, Tag},
};

/// A declared API operation.
///
/// Produced by the route attribute macros, which expand a handler function into
/// a zero-sized type implementing this trait. The type shadows the function
/// name, so `routes![get_user]` refers to the operation rather than the `fn`.
///
/// The builder form is public and supported for routes composed at runtime, but
/// the attribute is the recommended way: it takes the doc comment as the
/// operation's summary and description, and it can check the path template
/// against the handler's parameters at compile time, which the builder cannot.
pub trait Endpoint<C>: Send + Sync + 'static {
    /// The HTTP method.
    fn method(&self) -> Method;

    /// The path template, relative to any enclosing group.
    fn path(&self) -> &PathTemplate;

    /// Describes this operation, registering any schemas it needs.
    fn describe(&self, operation: &mut OperationCx<'_>);

    /// Handles a request.
    fn call(&self, request: Request, context: &C) -> impl Future<Output = Response> + Send;
}

/// The object-safe form of [`Endpoint`], so a router can hold a heterogeneous
/// set of them.
///
/// Private: boxing the future is how erasure is paid for, and no public
/// signature names a boxed future.
// Every method is called by `Router::build`, whose body is still `todo!()`.
#[allow(dead_code)]
pub(crate) trait DynEndpoint<C>: Send + Sync + 'static {
    fn method(&self) -> Method;

    fn path(&self) -> &PathTemplate;

    fn describe(&self, operation: &mut OperationCx<'_>);

    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;
}

impl<C: Send + Sync + 'static, E: Endpoint<C>> DynEndpoint<C> for E {
    fn method(&self) -> Method {
        Endpoint::method(self)
    }

    fn path(&self) -> &PathTemplate {
        Endpoint::path(self)
    }

    fn describe(&self, operation: &mut OperationCx<'_>) {
        Endpoint::describe(self, operation);
    }

    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(Endpoint::call(self, request, context))
    }
}

/// The compile-time facts a route attribute knows about an operation.
///
/// The attribute macros expand a handler into a zero-sized type implementing
/// this, which is what `routes!` collects. Everything here is `const`, so the
/// checks that depend on it — that path template variables match the handler's
/// path parameters, that no `operationId` repeats — happen during compilation
/// rather than at startup.
pub trait EndpointMeta {
    /// How this endpoint handles a panic while executing its operation.
    type PanicPolicy: PanicPolicy;

    /// The HTTP method, spelled as it appears on the wire.
    const METHOD: &'static str;

    /// The path template, relative to any enclosing group.
    const PATH: &'static str;

    /// The variable names appearing in [`PATH`](EndpointMeta::PATH).
    ///
    /// Compared against `PathParams::NAMES` by a const assertion in the
    /// expansion, so a handler whose parameters do not match its path is a
    /// compile error rather than a runtime 500.
    const PATH_VARIABLES: &'static [&'static str];

    /// The operation identifier.
    ///
    /// Defaults to the handler's module path and name, which is unique by
    /// construction.
    const OPERATION_ID: &'static str;

    /// The first line of the handler's doc comment.
    const SUMMARY: Option<&'static str>;

    /// The rest of the handler's doc comment.
    const DESCRIPTION: Option<&'static str>;

    /// Whether the handler carried `#[deprecated]`.
    const DEPRECATED: bool;
}

/// A set of operations waiting to be mounted.
///
/// What `routes![..]` produces, and what [`IntoEndpoints`] fills in. Opaque and
/// append-only: the prefix, the panic policy and the interceptors belong to
/// whatever is mounting, not to the endpoints, so there is nothing here for a
/// caller to reach into.
pub struct Endpoints<C> {
    endpoints: Vec<Arc<dyn DynEndpoint<C>>>,
}

impl<C> std::fmt::Debug for Endpoints<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoints")
            .field("len", &self.endpoints.len())
            .finish_non_exhaustive()
    }
}

impl<C> Default for Endpoints<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> Endpoints<C> {
    /// Creates an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            endpoints: Vec::new(),
        }
    }

    /// Adds one operation.
    pub fn push<E: Endpoint<C>>(&mut self, endpoint: E) -> &mut Self
    where
        C: Send + Sync + 'static,
    {
        self.endpoints.push(Arc::new(endpoint));
        self
    }

    /// The number of operations collected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.endpoints.len()
    }

    /// Whether no operation has been collected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.endpoints.is_empty()
    }

    /// Moves every operation out of `other` into this set.
    pub(crate) fn absorb(&mut self, other: Self) {
        self.endpoints.extend(other.endpoints);
    }
}

/// A value that can contribute operations to a router or a group.
///
/// Implemented for [`Endpoints`], for [`EndpointBuilder`], and for tuples,
/// arrays and vectors of those — which is what lets `routes![a, b, c]` be one
/// argument.
///
/// There is deliberately no blanket implementation over [`Endpoint`]: it would
/// conflict with every one of the container implementations, because a
/// downstream crate may implement `Endpoint` for a tuple of its own types and
/// coherence has to assume it will. A hand-written endpoint is mounted with one
/// line — `sink.push(self)` — which is a small price for `routes!` working at
/// all.
pub trait IntoEndpoints<C> {
    /// Appends these operations to `sink`.
    fn into_endpoints(self, sink: &mut Endpoints<C>);
}

impl<C> IntoEndpoints<C> for Endpoints<C> {
    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        sink.absorb(self);
    }
}

impl<C, T: IntoEndpoints<C>, const N: usize> IntoEndpoints<C> for [T; N] {
    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        for item in self {
            item.into_endpoints(sink);
        }
    }
}

impl<C, T: IntoEndpoints<C>> IntoEndpoints<C> for Vec<T> {
    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        for item in self {
            item.into_endpoints(sink);
        }
    }
}

/// Emits `IntoEndpoints` for one tuple arity.
macro_rules! tuple_endpoints {
    ($($member:ident),+) => {
        impl<C, $($member: IntoEndpoints<C>),+> IntoEndpoints<C> for ($($member,)+) {
            #[allow(non_snake_case)]
            fn into_endpoints(self, sink: &mut Endpoints<C>) {
                let ($($member,)+) = self;
                $( $member.into_endpoints(sink); )+
            }
        }
    };
}

tuple_endpoints!(A);
tuple_endpoints!(A, B);
tuple_endpoints!(A, B, C2);
tuple_endpoints!(A, B, C2, D);
tuple_endpoints!(A, B, C2, D, E);
tuple_endpoints!(A, B, C2, D, E, F);
tuple_endpoints!(A, B, C2, D, E, F, G);
tuple_endpoints!(A, B, C2, D, E, F, G, H);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L, M);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L, M, N);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L, M, N, O);
tuple_endpoints!(A, B, C2, D, E, F, G, H, I, J, K, L, M, N, O, P);

/// Builds an endpoint at runtime, rather than with a route attribute.
///
/// The attribute macros expand into this. Reach for it directly only when the
/// set of routes is not known at compile time; the attribute checks things this
/// cannot, notably that a handler's path parameters match its path template.
///
/// ```no_run
/// # use kynos::{openapi, router::endpoint::EndpointBuilder};
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
    /// # use kynos::{openapi, router::endpoint::EndpointBuilder};
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
    /// Defaults to the handler's module path and name, which is unique by
    /// construction. Override it only to keep a generated client's method name
    /// stable across a refactor.
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

    /// Merges an extra contribution into the operation's description.
    #[must_use]
    pub fn contribute(self, contribution: OperationContribution) -> Self {
        let _ = contribution;
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
    A: Send + Sync + 'static,
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
    A: Send + Sync + 'static,
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
