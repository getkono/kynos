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
mod route;

use std::{
    convert::Infallible,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use kynos_openapi::{Method, OpaqueRoute};

use crate::{
    error::problem::Problem,
    http::{Request, Response, StatusCode},
    middleware::erased::{ErasedInterceptor, ErasedTerminal},
    response::IntoResponse,
    router::{dispatch::Dispatch, service::Service},
};

/// What the router captured for `name`, percent-decoded.
///
/// The unchecked counterpart of
/// [`Path<T>`](crate::extract::params::path::Path): an undescribed route has
/// no template to decode into, so this hands back the text. `None` when the
/// request never passed the router, when the pattern declares no such
/// variable, or when the capture is not UTF-8.
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
    /// Interceptors the nested and merged routers it passed through mounted,
    /// outermost first. The root's own are added when the router is built.
    pub(crate) interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,
    /// Whether a router it passed through asked for panic recovery.
    pub(crate) catch_panics: bool,
    /// What the document records in place of a `paths` entry.
    pub(crate) record: OpaqueRoute,
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

/// Runs one operation's layers, outermost first, and then the operation.
///
/// Called rather than awaited in place, so the dispatch future awaiting the
/// box this returns holds no second copy of the request.
pub(crate) fn through_layers<C>(
    layers: &[Arc<dyn ErasedLayer>],
    dispatch: Arc<Dispatch<C>>,
    path: usize,
    position: usize,
    request: Request,
) -> BoxResponse
where
    C: Send + Sync + 'static,
{
    // Built inside out, so the outermost layer is called first.
    let mut next = Continuation::operation(dispatch, path, position);
    for layer in layers.iter().rev() {
        next = Continuation::through(Arc::clone(layer), next);
    }

    next.call(request)
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
    /// [`OpaqueReason::UntypedLayer`](kynos_openapi::OpaqueReason::UntypedLayer)
    /// at conversion time, because whatever
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
