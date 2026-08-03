//! Routing, grouping, and the path from code to description.

use kynos_openapi::{Document, Info, Method, PathTemplate, SpecVersion, Violation};

use crate::{
    error::Result,
    handler::Handler,
    middleware::{
        Interceptor, Observer, OperationContribution,
        catch_panic::{Catch, PanicPolicy, Propagate},
    },
    schema::Registry,
    security::SecurityScheme,
};

const PATH_SEGMENT_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// Builds a URI for a generated endpoint without dynamic parameters.
#[doc(hidden)]
pub fn endpoint_uri(template: &str) -> crate::http::Uri {
    template
        .parse()
        .expect("a route macro only emits a valid URI path")
}

/// Builds a URI for a generated endpoint with path parameters.
#[doc(hidden)]
pub fn endpoint_uri_with_path<P: crate::extract::PathParams>(
    template: &str,
    path: &P,
) -> crate::http::Uri {
    render_endpoint_path(template, path)
        .parse()
        .expect("derived path parameters produce a valid URI")
}

/// Builds a URI for a generated endpoint with query parameters.
#[doc(hidden)]
pub fn endpoint_uri_with_query<Q: crate::extract::QueryParams>(
    template: &str,
    query: &Q,
) -> crate::http::Uri {
    let query = query.encode();
    let uri = if query.is_empty() {
        template.to_owned()
    } else {
        format!("{template}?{query}")
    };
    uri.parse()
        .expect("derived query parameters produce a valid URI")
}

/// Builds a URI for a generated endpoint with path and query parameters.
#[doc(hidden)]
pub fn endpoint_uri_with_path_and_query<
    P: crate::extract::PathParams,
    Q: crate::extract::QueryParams,
>(
    template: &str,
    path: &P,
    query: &Q,
) -> crate::http::Uri {
    let path = render_endpoint_path(template, path);
    let query = query.encode();
    let uri = if query.is_empty() {
        path
    } else {
        format!("{path}?{query}")
    };
    uri.parse()
        .expect("derived endpoint parameters produce a valid URI")
}

fn render_endpoint_path<P: crate::extract::PathParams>(template: &str, path: &P) -> String {
    let values = path.encode();
    assert_eq!(
        values.len(),
        P::NAMES.len(),
        "PathParams::encode must return one value per declared name"
    );

    let mut rendered = template.to_owned();
    for (name, value) in values {
        assert!(
            P::NAMES.contains(&name),
            "PathParams::encode returned undeclared name `{name}`"
        );
        let encoded =
            percent_encoding::utf8_percent_encode(&value, PATH_SEGMENT_ENCODE_SET).to_string();
        rendered = rendered.replace(&format!("{{{name}}}"), &encoded);
    }
    rendered
}

/// Compares derived path parameter names with a route template in const code.
#[doc(hidden)]
pub const fn path_parameter_names_match(left: &[&str], right: &[&str]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if !const_str_eq(left[index], right[index]) {
            return false;
        }
        index += 1;
    }
    true
}

const fn const_str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

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
pub struct Group<C, P = Propagate> {
    _private: std::marker::PhantomData<(C, P)>,
}

impl<C> Group<C, Propagate> {
    /// Creates a group mounted at `prefix`.
    #[must_use]
    pub fn new(prefix: &'static str) -> Self {
        let _ = prefix;
        todo!()
    }
}

impl<C, P: PanicPolicy> Group<C, P> {
    /// Converts panics from covered operations into documented 500 responses.
    ///
    /// The policy is carried in the group's type and resolved while its
    /// endpoints are mounted. No recovery branch is installed when this method
    /// is not called.
    ///
    /// # Compile-time requirement
    ///
    /// The final binary must use `panic = "unwind"`. Selecting this policy in
    /// a `panic = "abort"` build is a compile-time error.
    ///
    /// ```no_run
    /// let users = kynos::router::Group::<()>::new("/users").catch_panics();
    /// # let _ = users;
    /// ```
    #[must_use]
    pub fn catch_panics(self) -> Group<C, Catch> {
        const {
            assert!(
                cfg!(panic = "unwind"),
                "Kynos panic recovery requires `panic = \"unwind\"`; remove `catch_panics` or enable unwinding"
            );
        }
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

impl<C, E: Endpoint<C>> IntoEndpoints<C> for E {
    fn add_to(self, router: &mut Router<C>) {
        let _ = (self, router);
        todo!()
    }
}

/// The root of an API.
///
/// `C` is the application context type — the dependency-injection container
/// every handler resolves its state from. A handler asking for something the
/// context does not provide is a compile error, not a runtime panic.
#[derive(Debug)]
pub struct Router<C, P = Propagate> {
    _private: std::marker::PhantomData<(C, P)>,
}

impl<C> Default for Router<C, Propagate> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> Router<C, Propagate> {
    /// Creates an empty router.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }
}

impl<C, P: PanicPolicy> Router<C, P> {
    /// Converts panics from covered operations into documented 500 responses.
    ///
    /// The policy is carried in the router's type and resolved when the service
    /// is built. No recovery branch is installed when this method is not
    /// called.
    ///
    /// # Compile-time requirement
    ///
    /// The final binary must use `panic = "unwind"`. Selecting this policy in
    /// a `panic = "abort"` build is a compile-time error.
    ///
    /// ```no_run
    /// let router = kynos::Router::<()>::new().catch_panics();
    /// # let _ = router;
    /// ```
    #[must_use]
    pub fn catch_panics(self) -> Router<C, Catch> {
        const {
            assert!(
                cfg!(panic = "unwind"),
                "Kynos panic recovery requires `panic = \"unwind\"`; remove `catch_panics` or enable unwinding"
            );
        }
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
    pub fn group<GP: PanicPolicy>(self, group: Group<C, GP>) -> Self {
        let _ = group;
        todo!()
    }

    /// Mounts another router beneath a path prefix.
    #[must_use]
    pub fn nest<NP: PanicPolicy>(self, prefix: &'static str, router: Router<C, NP>) -> Self {
        let _ = (prefix, router);
        todo!()
    }

    /// Merges another router at the same level.
    #[must_use]
    pub fn merge<OP: PanicPolicy>(self, router: Router<C, OP>) -> Self {
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

    /// Sets the application-wide trailing-slash policy.
    ///
    /// Redirect mode only adds or removes the final slash to reach an exactly
    /// declared path. It never changes path casing or normalizes individual
    /// routes, and uses 308 so the request method and body are preserved.
    #[must_use]
    pub fn trailing_slashes(self, policy: TrailingSlashPolicy) -> Self {
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
    /// Returns [`Error::Invalid`](crate::Error::Invalid) if the router cannot be described at all.
    pub fn validate(&self) -> Result<Vec<Violation>> {
        todo!()
    }

    /// Produces the OpenAPI description.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`](crate::Error::Invalid) when validation finds an error-level
    /// violation, so a misleading description is never emitted.
    pub fn openapi(&self) -> Result<Document> {
        todo!()
    }

    /// Produces the description targeting a specific specification version.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`](crate::Error::Invalid) on a validation error, or if the API uses a
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
    /// Returns [`Error::Invalid`](crate::Error::Invalid) with every violation found.
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

/// How the router handles a request differing only by a trailing slash.
///
/// ```no_run
/// use kynos::router::TrailingSlashPolicy;
///
/// let router = kynos::Router::<()>::new()
///     .trailing_slashes(TrailingSlashPolicy::Redirect);
/// # let _ = router;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrailingSlashPolicy {
    /// Treat the two paths as distinct and use the normal not-found policy.
    #[default]
    Strict,
    /// Redirect to the exactly declared path with status 308.
    Redirect,
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
///
/// ```no_run
/// # use kynos::{handler::Handler, http, openapi, router::EndpointBuilder, schema::Registry};
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
    /// # use kynos::{handler::Handler, http, openapi, router::EndpointBuilder, schema::Registry};
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

#[cfg(test)]
mod tests {
    use crate::extract::PathParams;

    struct Params;

    impl PathParams for Params {
        const NAMES: &'static [&'static str] = &["name"];

        fn encode(&self) -> Vec<(&'static str, String)> {
            vec![("name", "sales/2026 report".to_owned())]
        }
    }

    #[test]
    fn typed_endpoint_paths_percent_encode_each_segment() {
        let uri = super::endpoint_uri_with_path("/reports/{name}", &Params);
        assert_eq!(uri, "/reports/sales%2F2026%20report");
    }

    #[test]
    fn path_parameter_names_compare_in_const_context() {
        const MATCHES: bool =
            super::path_parameter_names_match(&["tenant", "id"], &["tenant", "id"]);
        const DIFFERS: bool =
            super::path_parameter_names_match(&["tenant", "id"], &["id", "tenant"]);
        assert!(std::hint::black_box(MATCHES));
        assert!(!std::hint::black_box(DIFFERS));
    }
}
