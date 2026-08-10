//! One declared operation: what a route attribute produces, and the builder it
//! expands into.

use kynos_openapi::{Method, PathTemplate};

use crate::{
    handler::Handler,
    middleware::{
        Interceptor,
        catch_panic::{Catch, PanicPolicy, Propagate},
        contribution::OperationContribution,
    },
    router::{Router, operation::Tag},
    schema::registry::Registry,
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
pub trait Endpoint<C> {
    /// The HTTP method.
    fn method(&self) -> Method;

    /// The path template, relative to any enclosing group.
    fn path(&self) -> &PathTemplate;

    /// Describes this operation, registering any schemas it needs.
    fn describe(&self, registry: &mut Registry) -> kynos_openapi::Operation;

    /// Handles a request.
    fn call(
        &self,
        request: crate::http::Request,
        context: &C,
    ) -> impl Future<Output = crate::http::Response> + Send;
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

/// A collection of endpoints, as produced by the `routes!` macro.
pub trait IntoEndpoints<C> {
    /// Adds these endpoints to a router under construction.
    fn add_to(self, router: &mut Router<C>);
}

impl<C, E: Endpoint<C>> IntoEndpoints<C> for E {
    fn add_to(self, router: &mut Router<C>) {
        let _ = (self, router);
        todo!()
    }
}

/// Builds an endpoint at runtime, rather than with a route attribute.
///
/// The attribute macros expand into this. Reach for it directly only when the
/// set of routes is not known at compile time; the attribute checks things this
/// cannot, notably that a handler's path parameters match its path template.
///
/// ```no_run
/// # use kynos::{
/// #     handler::Handler, http, openapi, router::endpoint::EndpointBuilder, schema::registry::Registry,
/// # };
/// # #[derive(Clone)] struct Health;
/// # impl Handler<()> for Health {
/// #     async fn call(self, _: http::Request, _: ()) -> http::Response { todo!() }
/// #     fn describe(_: &mut Registry) -> openapi::Operation { todo!() }
/// # }
/// let endpoint = EndpointBuilder::new(
///     openapi::Method::Get,
///     openapi::PathTemplate::parse("/health").expect("valid path"),
///     Health,
/// )
/// .intercept(kynos::middleware::limits::BodySize::new(1_024));
/// let router = kynos::Router::<()>::new().mount(endpoint);
/// # let _ = router;
/// ```
#[derive(Debug)]
pub struct EndpointBuilder<C, H, P = Propagate, I = ()> {
    _private: std::marker::PhantomData<(C, H, P, I)>,
}

impl<C, H: Handler<C>> EndpointBuilder<C, H, Propagate, ()> {
    /// Begins an endpoint for `handler`.
    #[must_use]
    pub fn new(method: Method, path: PathTemplate, handler: H) -> Self {
        let _ = (method, path, handler);
        todo!()
    }
}

impl<C, H: Handler<C>, P: PanicPolicy, I> EndpointBuilder<C, H, P, I> {
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
    /// # use kynos::{
    /// #     handler::Handler, http, openapi, router::endpoint::EndpointBuilder, schema::registry::Registry,
    /// # };
    /// # #[derive(Clone)]
    /// # struct Health;
    /// # impl Handler<()> for Health {
    /// #     async fn call(self, _: http::Request, _: ()) -> http::Response { todo!() }
    /// #     fn describe(_: &mut Registry) -> openapi::Operation { todo!() }
    /// # }
    /// let endpoint = EndpointBuilder::new(
    ///     openapi::Method::Get,
    ///     openapi::PathTemplate::parse("/health").expect("valid path"),
    ///     Health,
    /// )
    /// .catch_panics();
    /// # let _ = endpoint;
    /// ```
    #[must_use]
    pub fn catch_panics(self) -> EndpointBuilder<C, H, Catch, I> {
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
    /// description and its type is retained in the builder for runtime
    /// composition.
    #[must_use]
    pub fn intercept<N: Interceptor<C>>(self, interceptor: N) -> EndpointBuilder<C, H, P, (I, N)> {
        let _ = interceptor;
        todo!()
    }
}

impl<C, H, P, I> Endpoint<C> for EndpointBuilder<C, H, P, I>
where
    C: Sync,
    H: Handler<C>,
    I: Sync,
    P: PanicPolicy,
{
    fn method(&self) -> Method {
        todo!()
    }

    fn path(&self) -> &PathTemplate {
        todo!()
    }

    fn describe(&self, registry: &mut Registry) -> kynos_openapi::Operation {
        let _ = registry;
        todo!()
    }

    async fn call(&self, request: crate::http::Request, context: &C) -> crate::http::Response {
        let _ = (request, context);
        todo!()
    }
}
