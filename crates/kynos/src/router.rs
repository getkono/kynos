//! Routing, grouping, and the path from code to description.

use kynos_openapi::{Document, Info, Method, PathTemplate, SpecVersion, Violation};

use crate::{
    error::Result,
    handler::Handler,
    middleware::{Interceptor, Observer, OperationContribution},
    schema::Registry,
    security::SecurityScheme,
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

/// The description of the operation currently being built.
///
/// Passed to [`Describe`](crate::extract::Describe) implementations so that
/// each handler input can add its own parameters or request body.
#[derive(Debug)]
pub struct OperationCx<'a> {
    _private: std::marker::PhantomData<&'a ()>,
}

impl OperationCx<'_> {
    /// Adds a parameter to the operation.
    pub fn add_parameter(&mut self, parameter: kynos_openapi::Parameter) {
        let _ = parameter;
        todo!()
    }

    /// Sets the operation's request body.
    ///
    /// # Panics
    ///
    /// Panics if a request body was already set. The trait bounds make this
    /// unreachable from a handler — only one argument may implement
    /// [`FromRequest`](crate::extract::FromRequest) — so reaching it indicates
    /// a hand-written [`Describe`](crate::extract::Describe) implementation
    /// that claims a body it does not consume.
    pub fn set_request_body(&mut self, body: kynos_openapi::RequestBody) {
        let _ = body;
        todo!()
    }

    /// Adds a security requirement.
    pub fn add_security(&mut self, requirement: kynos_openapi::SecurityRequirement) {
        let _ = requirement;
        todo!()
    }

    /// Merges responses an input's rejection can produce.
    pub fn add_responses(&mut self, responses: kynos_openapi::Responses) {
        let _ = responses;
        todo!()
    }

    /// The registry, for describing a schema this input needs.
    pub fn registry(&mut self) -> &mut Registry {
        todo!()
    }
}

/// A tag, as a type.
///
/// Derived with `#[derive(Tag)]` on a unit struct. Making tags types rather
/// than strings means a typo is a compile error, and tag-name uniqueness is a
/// property of the module system rather than something checked afterwards.
pub trait Tag {
    /// The tag name as it appears in the description.
    const NAME: &'static str;

    /// The tag's metadata.
    fn metadata() -> kynos_openapi::Tag;
}

/// A set of operations sharing a path prefix, a tag, and interceptors.
///
/// This is the recommended unit of API structure: one group per resource. The
/// prefix becomes part of each path, the tag is applied to each operation, and
/// each interceptor's contribution is merged into each operation's description
/// — so attaching authentication to a group documents it on every operation
/// underneath, correctly, without anyone maintaining that by hand.
#[derive(Debug)]
pub struct Group<C> {
    _private: std::marker::PhantomData<C>,
}

impl<C> Group<C> {
    /// Creates a group mounted at `prefix`.
    #[must_use]
    pub fn new(prefix: &'static str) -> Self {
        let _ = prefix;
        todo!()
    }

    /// Tags every operation in this group.
    #[must_use]
    pub fn tag<T: Tag>(self) -> Self {
        todo!()
    }

    /// Applies an interceptor to every operation in this group.
    #[must_use]
    pub fn intercept<I: Interceptor<C>>(self, interceptor: I) -> Self {
        let _ = interceptor;
        todo!()
    }

    /// Mounts operations into this group.
    #[must_use]
    pub fn mount<E: IntoEndpoints<C>>(self, endpoints: E) -> Self {
        let _ = endpoints;
        todo!()
    }
}

/// A collection of endpoints, as produced by the `routes!` macro.
pub trait IntoEndpoints<C> {
    /// Adds these endpoints to a router under construction.
    fn add_to(self, router: &mut Router<C>);
}

/// The root of an API.
///
/// `C` is the application context type — the dependency-injection container
/// every handler resolves its state from. A handler asking for something the
/// context does not provide is a compile error, not a runtime panic.
#[derive(Debug)]
pub struct Router<C> {
    _private: std::marker::PhantomData<C>,
}

impl<C> Default for Router<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> Router<C> {
    /// Creates an empty router.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }

    /// Sets the description's `info` block.
    #[must_use]
    pub fn info(self, info: Info) -> Self {
        let _ = info;
        todo!()
    }

    /// Declares a server providing this API.
    ///
    /// Never inferred from the bind address: the description states the public
    /// URL clients use, which is usually not the socket the process listens on.
    #[must_use]
    pub fn server(self, server: kynos_openapi::Server) -> Self {
        let _ = server;
        todo!()
    }

    /// Mounts operations at the router's root.
    #[must_use]
    pub fn mount<E: IntoEndpoints<C>>(self, endpoints: E) -> Self {
        let _ = endpoints;
        todo!()
    }

    /// Mounts a group.
    #[must_use]
    pub fn group(self, group: Group<C>) -> Self {
        let _ = group;
        todo!()
    }

    /// Mounts another router beneath a path prefix.
    #[must_use]
    pub fn nest(self, prefix: &'static str, router: Self) -> Self {
        let _ = (prefix, router);
        todo!()
    }

    /// Merges another router at the same level.
    #[must_use]
    pub fn merge(self, router: Self) -> Self {
        let _ = router;
        todo!()
    }

    /// Declares a security scheme the API can use.
    #[must_use]
    pub fn security_scheme<S: SecurityScheme>(self) -> Self {
        todo!()
    }

    /// Registers tag metadata.
    #[must_use]
    pub fn tag<T: Tag>(self) -> Self {
        todo!()
    }

    /// Applies an interceptor to every operation in the router.
    #[must_use]
    pub fn intercept<I: Interceptor<C>>(self, interceptor: I) -> Self {
        let _ = interceptor;
        todo!()
    }

    /// Installs an observer.
    ///
    /// Observers see everything and change nothing, so they contribute nothing
    /// to the description. This is where request logging belongs.
    #[must_use]
    pub fn observe<O: Observer<C>>(self, observer: O) -> Self {
        let _ = observer;
        todo!()
    }

    /// Sets what happens when no route matches.
    ///
    /// Not a route: an unmatched path is outside the description entirely, and
    /// contributes no `paths` entry.
    #[must_use]
    pub fn not_found(self, policy: FallbackPolicy) -> Self {
        let _ = policy;
        todo!()
    }

    /// Sets what happens when a path matches but the method does not.
    ///
    /// The `Allow` header is derived from the operations actually declared on
    /// that path, so it cannot disagree with the description.
    #[must_use]
    pub fn method_not_allowed(self, policy: FallbackPolicy) -> Self {
        let _ = policy;
        todo!()
    }

    /// Turns unconstrained-schema warnings into build errors.
    ///
    /// [`Unchecked`](crate::schema::Unchecked) is honest but weak. A team that
    /// wants no weak schemas at all can say so here.
    #[must_use]
    pub fn deny_unchecked_schemas(self) -> Self {
        todo!()
    }

    /// Checks the router without building it.
    ///
    /// Returns every violation, including warnings. Worth an integration test:
    /// it catches the mistakes that only show up across a whole API — a
    /// duplicated `operationId`, two paths that differ only in variable name, a
    /// security requirement naming a scheme nobody declared.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] if the router cannot be described at all.
    pub fn validate(&self) -> Result<Vec<Violation>> {
        todo!()
    }

    /// Produces the OpenAPI description.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when validation finds an error-level
    /// violation, so a misleading description is never emitted.
    pub fn openapi(&self) -> Result<Document> {
        todo!()
    }

    /// Produces the description targeting a specific specification version.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] on a validation error, or if the API uses a
    /// construct `version` cannot express — a Server-Sent Events response
    /// requested as 3.1, say.
    pub fn openapi_as(&self, version: SpecVersion) -> Result<Document> {
        let _ = version;
        todo!()
    }

    /// Finalizes the router into something servable.
    ///
    /// This is where the structural checks run, so an API that cannot be
    /// described correctly fails at startup rather than at documentation time.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] with every violation found.
    pub fn build(self, context: C) -> Result<Service<C>> {
        let _ = context;
        todo!()
    }
}

/// What to return for a request no operation handles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FallbackPolicy {
    /// Reply with an RFC 9457 problem document. The default, so that a client
    /// meets one error shape across the whole service rather than two.
    #[default]
    Problem,
    /// Reply with an empty body and the status alone.
    Empty,
}

/// A built router, ready to serve.
#[derive(Debug)]
pub struct Service<C> {
    _private: std::marker::PhantomData<C>,
}

impl<C> Service<C> {
    /// The description of the API this service implements.
    #[must_use]
    pub fn openapi(&self) -> &Document {
        todo!()
    }

    /// Handles one request.
    ///
    /// Exposed so that a Kynos service can be driven directly — by a test, or
    /// by an embedding that owns its own accept loop.
    pub async fn call(&self, request: crate::http::Request) -> crate::http::Response {
        let _ = request;
        todo!()
    }
}

/// Builds an endpoint at runtime, rather than with a route attribute.
///
/// The attribute macros expand into this. Reach for it directly only when the
/// set of routes is not known at compile time; the attribute checks things this
/// cannot, notably that a handler's path parameters match its path template.
#[derive(Debug)]
pub struct EndpointBuilder<C, H> {
    _private: std::marker::PhantomData<(C, H)>,
}

impl<C, H: Handler<C>> EndpointBuilder<C, H> {
    /// Begins an endpoint for `handler`.
    #[must_use]
    pub fn new(method: Method, path: PathTemplate, handler: H) -> Self {
        let _ = (method, path, handler);
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
}
