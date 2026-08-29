//! Escape hatches, and what they cost.
//!
//! Everything in this module lets you build something Kynos cannot describe.
//! These are waivers: you are asserting that you know what the cost is.
//!
//! In exchange, `Router::validate` reports every unchecked construct, and the
//! operations a waiver reaches are emitted and flagged rather than dropped.
//! `x-kynos-document-not-authoritative` follows from that, stamped on the
//! document when any operation is flagged.
//!
//! That cost is deliberate and visible. A description that silently omits part
//! of the service is worse than no description, because consumers trust it. If
//! Kynos cannot describe something, it says so in the artifact rather than
//! quietly leaving a hole.
//!
//! The exception is [`Router::upgrade_unchecked`], because a connection that
//! has left HTTP has no vocabulary in any version of the specification — an
//! entry no consumer could act on would be worse than the honest absence, which
//! `Router::validate` reports either way.
//!
//! # When these are the right answer
//!
//! - A wildcard route serving static assets from the same binary, in a small
//!   deployment with no reverse proxy in front.
//! - A `tower` layer that has no equivalent interceptor yet, and that you have
//!   satisfied yourself is response-transparent.
//! - A WebSocket endpoint alongside a REST API. OpenAPI has no vocabulary for
//!   WebSockets at all — that is `AsyncAPI`'s domain — so this is not a gap Kynos
//!   can close later.
//!
//! # When they are not
//!
//! To avoid writing a type. Everything under [`crate::schema`] exists so that
//! the hard cases stay describable; reach for
//! [`Unchecked`](crate::schema::unchecked::Unchecked) before reaching for this module,
//! because a weak schema is still an honest one.

use std::{
    convert::Infallible,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use kynos_openapi::{Document, Method, OpaqueReason, OpaqueRoute};

use crate::{
    error::problem::Problem,
    http::{Request, Response, StatusCode},
    middleware::{catch_panic::PanicPolicy, erased::ErasedTerminal},
    response::IntoResponse,
    router::{Router, dispatch::Dispatch, group::Group, service::Service},
};

/// What the router captured for `name`, percent-decoded.
///
/// The one way an unchecked handler reads a wildcard. A described operation
/// takes a [`Path<T>`](crate::extract::params::path::Path) and gets a typed
/// value; there is no type here to decode into, because the whole point of the
/// waiver is that the route has no template — so this hands back the text.
///
/// Decoded, and bounded to the matched path, which is what makes it safe to
/// join onto anything: the value is a segment the matcher took apart rather
/// than a substring a handler sliced out of the URL by eye. `None` when the
/// pattern declares no such variable, or when the capture is not UTF-8.
///
/// ```no_run
/// use kynos::http::{Request, Response};
///
/// async fn serve(request: Request) -> Response {
///     let path = kynos::unchecked::captured(&request, "path");
///     todo!("resolve {path:?} against a directory")
/// }
/// ```
#[must_use]
pub fn captured<'r>(request: &'r Request, name: &str) -> Option<std::borrow::Cow<'r, str>> {
    let captures = request
        .extensions()
        .get::<crate::extract::params::path::PathCaptures>()?;

    let raw = captures.get(request.uri().path(), name)?;
    crate::__private::uri::decode_path_value(raw).ok()
}

/// A boxed response future.
///
/// The one place in the framework where a boxed future is sanctioned, and it is
/// confined to this module: `tower`'s `Service::Future` is an associated type,
/// so a service composed out of layers this crate cannot name has no other
/// shape to take.
type BoxResponse = Pin<Box<dyn Future<Output = Response> + Send>>;

/// A handler for a route Kynos does not describe.
///
/// Deliberately not [`Handler`](crate::handler::Handler): a described handler
/// takes described inputs, and the point of this hatch is that there are none.
/// It gets the request; it must produce a response.
pub trait UncheckedHandler<C>: Send + Sync + 'static {
    /// Handles a request that matched an undescribed route.
    fn call(
        &self,
        request: crate::http::Request,
        context: &C,
    ) -> impl Future<Output = crate::http::Response> + Send;
}

/// Any `async fn(Request) -> Response` is an unchecked handler.
impl<C, F, Fut> UncheckedHandler<C> for F
where
    C: Sync + 'static,
    F: Fn(crate::http::Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = crate::http::Response> + Send,
{
    fn call(
        &self,
        request: crate::http::Request,
        _context: &C,
    ) -> impl Future<Output = crate::http::Response> + Send {
        self(request)
    }
}

/// The rest of a request's path through Kynos, carried in its extensions.
///
/// A `tower::Layer` composes with a service *value*, and the only value Kynos
/// can hand it is an [`UncheckedInner`] built long before any request exists —
/// so the stand-in holds nothing and the continuation travels with the request
/// instead. [`UncheckedInner::call`] takes it back out.
#[derive(Clone)]
pub(crate) struct Continuation(Arc<dyn Fn(Request) -> BoxResponse + Send + Sync>);

impl Continuation {
    /// The continuation that runs one already-routed operation.
    fn operation<C>(dispatch: Arc<Dispatch<C>>, path: usize, position: usize) -> Self
    where
        C: Send + Sync + 'static,
    {
        Self(Arc::new(move |request| {
            Arc::clone(&dispatch).resume(path, position, request)
        }))
    }

    /// The continuation that runs `layer`, and then `inner`.
    fn through(layer: Arc<dyn ErasedLayer>, inner: Self) -> Self {
        Self(Arc::new(move |mut request| {
            request.extensions_mut().insert(inner.clone());
            layer.run(request)
        }))
    }

    fn call(&self, request: Request) -> BoxResponse {
        (self.0)(request)
    }
}

/// A `tower` layer applied to [`UncheckedInner`], with its type erased.
///
/// A router holds these beside the interceptors it can name. Nothing in the
/// description is derived from one — that is what the waiver waives — so the
/// trait needs only the ability to run.
pub(crate) trait ErasedLayer: Send + Sync + 'static {
    /// Drives one request through the layered service.
    ///
    /// The continuation is already in the request's extensions.
    fn run(&self, request: Request) -> BoxResponse;
}

/// One layered `tower` service, ready to be cloned per request.
struct Layered<S>(S);

impl<S> ErasedLayer for Layered<S>
where
    S: tower_service::Service<Request, Response = Response, Error = Infallible>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    fn run(&self, request: Request) -> BoxResponse {
        // Cloned per request because `tower` drives a service through `&mut
        // self`, while the router holds one shared instance of it.
        let mut service = self.0.clone();

        Box::pin(async move {
            match std::future::poll_fn(|context| service.poll_ready(context)).await {
                Ok(()) => {}
                Err(never) => match never {},
            }

            match service.call(request).await {
                Ok(response) => response,
                Err(never) => match never {},
            }
        })
    }
}

/// Applies `layer` to the stand-in service and erases what comes back.
fn erase<C, L>(layer: &L) -> Arc<dyn ErasedLayer>
where
    C: Send + Sync + 'static,
    L: tower::Layer<UncheckedInner<C>> + Send + Sync + 'static,
    L::Service: tower_service::Service<Request, Response = Response, Error = Infallible>
        + Clone
        + Send
        + Sync
        + 'static,
    <L::Service as tower_service::Service<Request>>::Future: Send + 'static,
{
    Arc::new(Layered(layer.layer(UncheckedInner::new())))
}

/// An [`UncheckedHandler`] as the end of a chain.
struct UncheckedTerminal<C, H> {
    handler: H,
    _context: PhantomData<fn() -> C>,
}

impl<C, H> ErasedTerminal<C> for UncheckedTerminal<C, H>
where
    C: Sync + 'static,
    H: UncheckedHandler<C>,
{
    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(self.handler.call(request, context))
    }
}

/// One route the description cannot express, and what serves it.
pub(crate) struct UncheckedRoute<C> {
    /// The router's matching pattern, with every enclosing prefix applied.
    pub(crate) pattern: String,
    /// The methods served, in declaration order.
    pub(crate) methods: Vec<Method>,
    pub(crate) terminal: Arc<dyn ErasedTerminal<C>>,
    /// Layers the enclosing scopes contributed, outermost first.
    pub(crate) layers: Vec<Arc<dyn ErasedLayer>>,
    /// What the document records in place of a `paths` entry.
    pub(crate) record: OpaqueRoute,
}

impl<C> UncheckedRoute<C> {
    /// Moves this route under `prefix`, keeping its record in step.
    ///
    /// A plain join, because the pattern is the router's matching syntax rather
    /// than a path template — there is nothing here to normalize beyond the one
    /// slash both halves would otherwise contribute.
    fn reprefix(&mut self, prefix: &str) {
        let prefix = prefix.strip_suffix('/').unwrap_or(prefix);
        if prefix.is_empty() {
            return;
        }

        self.pattern.insert_str(0, prefix);
        self.record.pattern.clone_from(&self.pattern);
        self.record.prefix = anchor(&self.pattern);
    }
}

/// Everything one scope holds that Kynos does not describe.
pub(crate) struct Unchecked<C> {
    /// Routes with no expressible path template, in declaration order.
    pub(crate) routes: Vec<UncheckedRoute<C>>,
    /// Layers covering every operation in this scope, outermost first.
    pub(crate) layers: Vec<Arc<dyn ErasedLayer>>,
}

// Hand-written for the reason `UncheckedInner`'s `Clone` is: a derive would
// bound it on `C`, and neither field needs anything of it.
impl<C> Default for Unchecked<C> {
    fn default() -> Self {
        Self {
            routes: Vec::new(),
            layers: Vec::new(),
        }
    }
}

impl<C> Unchecked<C> {
    /// Whether any waiver was taken here.
    pub(crate) fn is_empty(&self) -> bool {
        self.routes.is_empty() && self.layers.is_empty()
    }

    /// Takes over another scope's waivers, under `prefix`.
    ///
    /// The absorbed scope's layers covered exactly its own routes, so they
    /// become part of what each route carries rather than of what this scope
    /// applies to everything — the same rule interceptors follow.
    pub(crate) fn absorb(&mut self, other: Self, prefix: &str) {
        let Self { routes, layers } = other;

        for mut route in routes {
            route.reprefix(prefix);
            let mut covering = layers.clone();
            covering.append(&mut route.layers);
            route.layers = covering;
            self.routes.push(route);
        }
    }

    /// Records every unexpressible route on the document, and restamps it.
    pub(crate) fn annotate(&self, document: &mut Document) {
        for route in &self.routes {
            // The only reachable failure is a list already present in a shape
            // Kynos never emits, which a document Kynos just built cannot carry.
            let _ = route.record.append_to(document);
        }

        // Derived rather than set: the stamp summarizes what the document now
        // says, in both directions.
        document.restamp_authority();
    }
}

/// The literal prefix a pattern is anchored at, when it has variables past it.
///
/// A pattern with no variable at all is its own prefix, and restating it would
/// be noise rather than a second fact.
fn anchor(pattern: &str) -> Option<String> {
    let literal: Vec<&str> = pattern
        .split('/')
        .take_while(|segment| !segment.contains('{'))
        .collect();

    (literal.len() < pattern.split('/').count() && literal.len() > 1)
        .then(|| literal.join("/"))
        .filter(|prefix| !prefix.is_empty())
}

/// Whether a pattern could have been a path template after all.
///
/// A catch-all cannot, and neither can a segment carrying two variables — the
/// two shapes `docs/routing.md` records the router as declining. Anything else
/// reaching this module is undescribable because of its *handler*, which is a
/// different reason and is recorded as one.
fn expressible(pattern: &str) -> bool {
    pattern
        .split('/')
        .all(|segment| !segment.contains("{*") && segment.matches('{').count() <= 1)
}

/// The methods a route serves, and the ones OpenAPI has no field for.
///
/// A method in the second list is neither served nor claimed: the description
/// says only what the service honours, and the note on the record says the rest.
fn wire_methods<I: IntoIterator<Item = crate::http::Method>>(
    methods: I,
) -> (Vec<Method>, Vec<String>) {
    let mut served: Vec<Method> = Vec::new();
    let mut unmodelled: Vec<String> = Vec::new();

    for method in methods {
        match Method::from_wire_str(method.as_str()) {
            Some(method) if !served.contains(&method) => served.push(method),
            Some(_) => {}
            None => unmodelled.push(method.as_str().to_owned()),
        }
    }

    (served, unmodelled)
}

/// Runs one operation's layers, outermost first, and then the operation.
pub(crate) async fn through_layers<C>(
    layers: &[Arc<dyn ErasedLayer>],
    dispatch: Arc<Dispatch<C>>,
    path: usize,
    position: usize,
    request: Request,
) -> Response
where
    C: Send + Sync + 'static,
{
    // Built inside out, so that the outermost layer is the one called first and
    // the innermost is the one holding the operation.
    let mut next = Continuation::operation(dispatch, path, position);
    for layer in layers.iter().rev() {
        next = Continuation::through(Arc::clone(layer), next);
    }

    next.call(request).await
}

/// What a layer that discarded the request gets in place of a response.
///
/// Deliberately a 500: the layer is outside the description, so answering as
/// though the handler had run would be inventing an outcome.
fn lost_continuation() -> Response {
    Problem::new(StatusCode::INTERNAL_SERVER_ERROR).into_response()
}

/// The service an unchecked `tower` layer wraps.
///
/// Opaque on purpose: a layer may compose with it, but nothing in an
/// application may name a field of it or construct one.
pub struct UncheckedInner<C> {
    _private: std::marker::PhantomData<fn() -> C>,
}

impl<C> UncheckedInner<C> {
    /// The stand-in every unchecked layer is built around.
    fn new() -> Self {
        Self {
            _private: PhantomData,
        }
    }
}

// Hand-written: a derive would bound each on `C`, and `PhantomData<fn() -> C>`
// needs nothing of it. Most tower layers implement `Clone` for their service
// only when the inner one is, so a derived bound here would shut an
// application whose context is not `Clone` out of the hatch for no reason.
impl<C> Clone for UncheckedInner<C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C> Copy for UncheckedInner<C> {}

impl<C> std::fmt::Debug for UncheckedInner<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UncheckedInner").finish_non_exhaustive()
    }
}

impl<C> tower_service::Service<crate::http::Request> for UncheckedInner<C>
where
    C: Send + Sync + 'static,
{
    type Response = crate::http::Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _ = context;
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut request: crate::http::Request) -> Self::Future {
        // The continuation is put in place immediately before the layer is
        // invoked, so its absence means the layer answered with a request of
        // its own making and the rest of the chain is unreachable from here.
        let Some(continuation) = request.extensions_mut().remove::<Continuation>() else {
            return Box::pin(std::future::ready(Ok(lost_continuation())));
        };

        Box::pin(async move { Ok(continuation.call(request).await) })
    }
}

/// A Kynos service exposed through Tower's untyped service contract.
///
/// Creating this wrapper marks the OpenAPI document non-authoritative because
/// Tower layers can change responses in ways their types do not declare. The
/// normal [`Service`] intentionally does not implement `tower_service::Service`.
#[derive(Clone, Debug)]
pub struct UncheckedService<C> {
    service: Arc<Service<C>>,
}

impl<C> UncheckedService<C> {
    /// Returns the document, with every operation flagged opaque.
    #[must_use]
    pub fn openapi(&self) -> &kynos_openapi::Document {
        self.service.openapi()
    }
}

impl<C> Service<C> {
    /// Converts this service into an explicitly unchecked Tower service.
    ///
    /// Every operation in the document is flagged
    /// [`OpaqueReason::UntypedLayer`]
    /// at conversion time, because whatever ends up wrapping the returned
    /// service is outside the description and there is no way to know what it
    /// does.
    ///
    /// ```no_run
    /// # use kynos::{router::service::Service, unchecked::UncheckedService};
    /// fn tower<C: Send + Sync + 'static>(service: Service<C>) -> UncheckedService<C> {
    ///     service.into_tower_unchecked()
    /// }
    /// ```
    #[must_use]
    pub fn into_tower_unchecked(mut self) -> UncheckedService<C> {
        self.mark_opaque(kynos_openapi::OpaqueReason::UntypedLayer);
        UncheckedService {
            service: Arc::new(self),
        }
    }
}

impl<C> tower_service::Service<crate::http::Request> for UncheckedService<C>
where
    C: Send + Sync + 'static,
{
    type Response = crate::http::Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _ = context;
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: crate::http::Request) -> Self::Future {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.call(request).await) })
    }
}

impl<C, P: PanicPolicy, I, S> Router<C, P, I, S> {
    /// Wraps the router in an arbitrary `tower` layer.
    ///
    /// A `Layer` may change the status, rewrite the body, add headers, or
    /// refuse the request, and nothing in its type says which — so every
    /// operation underneath becomes a claim Kynos can no longer stand behind.
    ///
    /// Prefer writing an [`Interceptor`](crate::middleware::Interceptor). It is
    /// barely more work: name the responses it can answer with, the headers it
    /// adds and the headers it reads as its three associated types, and in
    /// return every covered operation documents it correctly and automatically
    /// — and two interceptors that would collide stop compiling.
    /// Every operation in this router's subtree is flagged
    /// [`OpaqueReason::UntypedLayer`],
    /// and nothing outside it is.
    ///
    /// The layer runs per-operation, after routing, exactly as an interceptor
    /// does — so a request that matched no route never reaches it, and there is
    /// no described operation for it to have invalidated.
    #[must_use]
    pub fn layer_unchecked<L>(mut self, layer: L) -> Self
    where
        C: Send + Sync + 'static,
        L: tower::Layer<UncheckedInner<C>> + Send + Sync + 'static,
        L::Service: tower_service::Service<
                crate::http::Request,
                Response = crate::http::Response,
                Error = Infallible,
            > + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as tower_service::Service<crate::http::Request>>::Future: Send + 'static,
    {
        self.unchecked.layers.push(erase::<C, L>(&layer));
        self
    }

    /// Adds a route whose path Kynos cannot express.
    ///
    /// Chiefly wildcards. A path parameter value must not contain an unescaped
    /// `/`, so `/assets/{*path}` has no OpenAPI equivalent — which is also why
    /// serving a directory tree, or an SPA fallback, is out of scope for the
    /// core.
    ///
    /// For anything beyond a handful of files, a reverse proxy or CDN is the
    /// better answer, and leaves the description intact.
    ///
    /// The route is recorded under
    /// [`OPAQUE_ROUTES_ANNOTATION`](kynos_openapi::annotation::OPAQUE_ROUTES_ANNOTATION)
    /// and the document is stamped non-authoritative. It gets no `paths` entry:
    /// no path template is true of a catch-all, so every key that could be
    /// minted would be a claim about either the path or a parameter that the
    /// service does not honour.
    ///
    /// The pattern is the router's own matching syntax, and the route is served
    /// from the same table as every described one — including the interceptors
    /// mounted on this router, which run here as they do everywhere else. Only
    /// a method OpenAPI has a field for can be served: one it does not is
    /// neither routed nor claimed, and the record says which were dropped.
    #[must_use]
    pub fn route_unchecked<M, H>(mut self, methods: M, pattern: &str, handler: H) -> Self
    where
        C: Sync + 'static,
        M: IntoIterator<Item = crate::http::Method>,
        H: UncheckedHandler<C>,
    {
        let (methods, unmodelled) = wire_methods(methods);
        let reason = if expressible(pattern) {
            // The pattern is a legal template, so what is undescribable here is
            // the handler rather than the path.
            OpaqueReason::UntypedHandler
        } else {
            OpaqueReason::UntypedRoute
        };

        let mut record = OpaqueRoute::new(pattern, reason)
            .with_methods(methods.iter().map(|method| method.as_wire_str()));
        if let Some(prefix) = anchor(pattern) {
            record = record.with_prefix(prefix);
        }
        if !unmodelled.is_empty() {
            record = record.with_note(format!(
                "not served: {} has no Path Item field, so Kynos will not route it",
                unmodelled.join(", ")
            ));
        }

        self.unchecked.routes.push(UncheckedRoute {
            pattern: pattern.to_owned(),
            methods,
            terminal: Arc::new(UncheckedTerminal {
                handler,
                _context: PhantomData,
            }),
            layers: Vec::new(),
            record,
        });
        self
    }

    /// Records one route with a reason of Kynos's own choosing.
    ///
    /// The seam `assets_directory` is built on. Not public: the reason has to
    /// come from the closed set in [`OpaqueReason`], and a caller free to
    /// invent one could describe a waiver the validator has no rule for.
    /// `route_unchecked` is the public door, and it derives the reason from the
    /// pattern.
    // Gated to the caller rather than to `unchecked`, which is the wider door:
    // `assets-fs` implies `unchecked`, so a build that takes the escape hatch
    // without the directory server carries this for nothing.
    #[cfg(feature = "assets-fs")]
    #[must_use]
    pub(crate) fn record_unchecked_route<H>(
        mut self,
        pattern: String,
        record: OpaqueRoute,
        handler: H,
    ) -> Self
    where
        C: Sync + 'static,
        H: UncheckedHandler<C>,
    {
        self.unchecked.routes.push(UncheckedRoute {
            pattern,
            methods: vec![Method::Get],
            terminal: Arc::new(UncheckedTerminal {
                handler,
                _context: PhantomData,
            }),
            layers: Vec::new(),
            record,
        });
        self
    }

    /// Adds a route that upgrades the connection away from HTTP.
    ///
    /// WebSockets, chiefly. This is not a temporary gap: OpenAPI describes HTTP
    /// request/response semantics, and a socket that stops being either is
    /// outside what any version of the specification can express. `AsyncAPI`
    /// covers this ground, and Kynos would rather point at it than pretend.
    ///
    /// Served on `GET`, which is the only method [RFC 9110][] leaves an upgrade
    /// handshake — and the only one RFC 6455 permits — and recorded with
    /// [`OpaqueReason::ProtocolUpgrade`], which is a different reason from a
    /// catch-all's and not one that will stop applying.
    ///
    /// [RFC 9110]: https://www.rfc-editor.org/rfc/rfc9110
    #[must_use]
    pub fn upgrade_unchecked<H>(mut self, path: &str, handler: H) -> Self
    where
        C: Sync + 'static,
        H: UncheckedHandler<C>,
    {
        let mut record = OpaqueRoute::new(path, OpaqueReason::ProtocolUpgrade)
            .with_methods([Method::Get.as_wire_str()])
            .with_note("the connection leaves HTTP, which no version of the specification models");
        if let Some(prefix) = anchor(path) {
            record = record.with_prefix(prefix);
        }

        self.unchecked.routes.push(UncheckedRoute {
            pattern: path.to_owned(),
            methods: vec![Method::Get],
            terminal: Arc::new(UncheckedTerminal {
                handler,
                _context: PhantomData,
            }),
            layers: Vec::new(),
            record,
        });
        self
    }

    /// Whether anything unchecked has been added.
    ///
    /// When true, the emitted document carries
    /// `x-kynos-document-not-authoritative`.
    ///
    /// [`examples/unchecked.rs`] presents `assert!(!router.has_unchecked())` as
    /// the line a CI job asserts on, and that is what it is for. Reach for
    /// [`unchecked_reasons`](Self::unchecked_reasons) where a service takes one
    /// waiver deliberately and wants the gate to keep holding for everything
    /// else.
    ///
    /// [`examples/unchecked.rs`]: https://github.com/getkono/kynos/blob/master/crates/kynos/examples/unchecked.rs
    #[must_use]
    pub fn has_unchecked(&self) -> bool {
        !self.unchecked.is_empty()
            || self
                .mounted
                .iter()
                .any(|mounted| !mounted.unchecked_layers.is_empty())
    }

    /// Every reason a waiver has been taken in this router, deduplicated.
    ///
    /// The gate [`has_unchecked`](Self::has_unchecked) cannot be. A service
    /// that serves a directory of static files has taken one waiver on purpose;
    /// asserting `!has_unchecked()` there means deleting the assertion for
    /// *everything*, which is how a check meant to catch an accidental
    /// `layer_unchecked` stops catching one.
    ///
    /// ```no_run
    /// # use kynos::{Router, openapi::OpaqueReason};
    /// # fn router() -> Router<()> { todo!() }
    /// // Anything waived except a file tree is a mistake.
    /// assert_eq!(router().unchecked_reasons(), [OpaqueReason::StaticAssets]);
    /// ```
    ///
    /// The order is the order the waivers were taken, which is stable for one
    /// router and is not something to depend on across two.
    #[must_use]
    pub fn unchecked_reasons(&self) -> Vec<OpaqueReason> {
        let mut reasons: Vec<OpaqueReason> = Vec::new();

        let mut record = |reason: OpaqueReason| {
            if !reasons.contains(&reason) {
                reasons.push(reason);
            }
        };

        for route in &self.unchecked.routes {
            record(route.record.reason.clone());
        }
        if !self.unchecked.layers.is_empty()
            || self
                .mounted
                .iter()
                .any(|mounted| !mounted.unchecked_layers.is_empty())
        {
            record(OpaqueReason::UntypedLayer);
        }

        reasons
    }
}

impl<C, P: PanicPolicy, I, S> Group<C, P, I, S> {
    /// Wraps this group in an arbitrary `tower` layer.
    ///
    /// Flags exactly this group's operations, and nothing else. One unchecked
    /// layer on one subtree must not taint three hundred operations it never
    /// touches.
    #[must_use]
    pub fn layer_unchecked<L>(mut self, layer: L) -> Self
    where
        C: Send + Sync + 'static,
        L: tower::Layer<UncheckedInner<C>> + Send + Sync + 'static,
        L::Service: tower_service::Service<
                crate::http::Request,
                Response = crate::http::Response,
                Error = Infallible,
            > + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as tower_service::Service<crate::http::Request>>::Future: Send + 'static,
    {
        self.unchecked_layers.push(erase::<C, L>(&layer));
        self
    }
}
