//! Escape hatches, and what they cost.
//!
//! Everything in this module lets you build something Kynos cannot describe.
//! These are waivers: you are asserting that you know what the cost is.
//!
//! In exchange, `Router::validate` reports every unchecked construct, and the
//! operations a waiver reaches are emitted and flagged rather than dropped.
//! `x-kynos-document-not-authoritative` follows from that, stamped on the
//! document when any operation is flagged. If Kynos cannot describe something,
//! it says so in the artifact rather than quietly leaving a hole.
//!
//! The exception is [`Router::upgrade_unchecked`]: a connection that has left
//! HTTP has no vocabulary in any version of the specification, so it gets no
//! entry, though `Router::validate` still reports it.
//!
//! # When these are the right answer
//!
//! - A wildcard route serving static assets from the same binary, in a small
//!   deployment with no reverse proxy in front.
//! - A `tower` layer that has no equivalent interceptor yet, and that you have
//!   satisfied yourself is response-transparent.
//! - A WebSocket or WebTransport endpoint alongside a REST API. OpenAPI has no
//!   vocabulary for either — that is `AsyncAPI`'s domain — so this is not a gap
//!   Kynos can close later.
//!
//! # When they are not
//!
//! To avoid writing a type. Everything under [`crate::schema`] exists so that
//! the hard cases stay describable; reach for
//! [`Unchecked`](crate::schema::unchecked::Unchecked) before reaching for this module,
//! because a weak schema is still an honest one.

mod describe;

use std::{
    convert::Infallible,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use kynos_openapi::{Method, OpaqueReason, OpaqueRoute};

use crate::{
    error::problem::Problem,
    http::{Request, Response, StatusCode},
    middleware::{catch_panic::PanicPolicy, erased::ErasedTerminal},
    response::IntoResponse,
    router::{Router, dispatch::Dispatch, group::Group, service::Service},
};

/// What the router captured for `name`, percent-decoded.
///
/// The unchecked counterpart of
/// [`Path<T>`](crate::extract::params::path::Path): an undescribed route has
/// no template to decode into, so this hands back the text. `None` when the
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
        .get::<crate::router::dispatch::Routed>()?
        .captures
        .as_ref()?;

    let raw = captures.get(request.uri().path(), name)?;
    crate::__private::uri::decode_path_value(raw).ok()
}

/// A boxed response future, sanctioned only here: a service composed of layers
/// this crate cannot name has no other `Service::Future` shape.
type BoxResponse = Pin<Box<dyn Future<Output = Response> + Send>>;

/// A handler for a route Kynos does not describe.
///
/// Unlike a [`Handler`](crate::handler::Handler), it takes no described inputs:
/// it gets the request and must produce a response.
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
/// The [`UncheckedInner`] a layer wraps is built before any request exists, so
/// the continuation travels with the request and `UncheckedInner::call` takes
/// it back out.
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

/// A `tower` layer applied to [`UncheckedInner`], with its type erased. Nothing
/// in the description derives from one, so it only needs to run.
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
        // `tower` drives a service through `&mut self`; the router holds one.
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
    L: tower_layer::Layer<UncheckedInner<C>> + Send + Sync + 'static,
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
    /// Moves this route under `prefix`, keeping its record in step. A plain
    /// join: the pattern is matching syntax, not a path template.
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

// Hand-written so it is not bounded on `C`.
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
    /// The absorbed scope's layers become part of each of its routes, since
    /// they covered exactly those.
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
}

/// The literal prefix a pattern is anchored at, when it has variables past it.
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
/// A catch-all cannot, and neither can a segment carrying two variables.
fn expressible(pattern: &str) -> bool {
    pattern
        .split('/')
        .all(|segment| !segment.contains("{*") && segment.matches('{').count() <= 1)
}

/// The methods a route serves, and the ones OpenAPI has no field for (which
/// are not served).
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
    // Built inside out, so the outermost layer is called first.
    let mut next = Continuation::operation(dispatch, path, position);
    for layer in layers.iter().rev() {
        next = Continuation::through(Arc::clone(layer), next);
    }

    next.call(request).await
}

/// The 500 a layer that discarded the request gets in place of a response.
fn lost_continuation() -> Response {
    Problem::new(StatusCode::INTERNAL_SERVER_ERROR).into_response()
}

/// The service an unchecked `tower` layer wraps. Opaque: a layer may compose
/// with it, but an application cannot construct one.
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

// Hand-written so they are not bounded on `C`: tower layers are usually `Clone`
// only when their inner service is, which must not require `C: Clone`.
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
        // Absent only if the layer forwarded a request of its own making.
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

impl<C> Service<C> {
    /// Converts this service into an explicitly unchecked Tower service.
    ///
    /// Every operation in the document is flagged
    /// [`OpaqueReason::UntypedLayer`] at conversion time, because whatever
    /// wraps the returned service is outside the description.
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
    /// refuse the request, and nothing in its type says which. Every operation
    /// in this router's subtree is flagged [`OpaqueReason::UntypedLayer`], and
    /// nothing outside it is.
    ///
    /// Prefer writing an [`Interceptor`](crate::middleware::Interceptor), which
    /// names its responses and headers as associated types so every covered
    /// operation documents it.
    ///
    /// The layer runs per-operation, after routing, exactly as an interceptor
    /// does, so a request that matched no route never reaches it.
    #[must_use]
    pub fn layer_unchecked<L>(mut self, layer: L) -> Self
    where
        C: Send + Sync + 'static,
        L: tower_layer::Layer<UncheckedInner<C>> + Send + Sync + 'static,
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
    /// `/`, so `/assets/{*path}` has no OpenAPI equivalent. For anything beyond
    /// a handful of files, a reverse proxy or CDN is the better answer.
    ///
    /// The route is recorded under
    /// [`OPAQUE_ROUTES_ANNOTATION`](kynos_openapi::annotation::OPAQUE_ROUTES_ANNOTATION),
    /// gets no `paths` entry, and the document is stamped non-authoritative.
    ///
    /// The pattern is the router's own matching syntax, and the route runs the
    /// interceptors mounted on this router like any other. A method OpenAPI
    /// has no field for is not served, and the record says which were dropped.
    #[must_use]
    pub fn route_unchecked<M, H>(mut self, methods: M, pattern: &str, handler: H) -> Self
    where
        C: Sync + 'static,
        M: IntoIterator<Item = crate::http::Method>,
        H: UncheckedHandler<C>,
    {
        let (methods, unmodelled) = wire_methods(methods);
        let reason = if expressible(pattern) {
            // A legal template: the handler, not the path, is undescribable.
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
    /// The seam `assets_directory` is built on; `route_unchecked` is the public
    /// door, deriving the reason from the pattern.
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
    /// WebSockets chiefly, and WebTransport. OpenAPI describes HTTP
    /// request/response semantics, so this is not a temporary gap; `AsyncAPI`
    /// covers this ground.
    ///
    /// Served on `GET`, the only method [RFC 9110][] leaves an upgrade handshake
    /// and the only one RFC 6455 permits, and recorded with
    /// [`OpaqueReason::ProtocolUpgrade`].
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
    /// [`examples/unchecked.rs`] asserts `!router.has_unchecked()` in CI. Reach
    /// for [`unchecked_reasons`](Self::unchecked_reasons) where a service takes
    /// one waiver deliberately.
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
    /// Lets a service that takes one waiver on purpose keep gating the rest,
    /// which [`has_unchecked`](Self::has_unchecked) cannot.
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
    /// Flags exactly this group's operations, and nothing else.
    #[must_use]
    pub fn layer_unchecked<L>(mut self, layer: L) -> Self
    where
        C: Send + Sync + 'static,
        L: tower_layer::Layer<UncheckedInner<C>> + Send + Sync + 'static,
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
