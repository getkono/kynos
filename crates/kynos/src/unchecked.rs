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
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crate::router::{Router, service::Service};

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

/// The service an unchecked `tower` layer wraps.
///
/// Opaque on purpose: a layer may compose with it, but nothing in an
/// application may name a field of it or construct one.
pub struct UncheckedInner<C> {
    _private: std::marker::PhantomData<fn() -> C>,
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

    fn call(&mut self, request: crate::http::Request) -> Self::Future {
        let _ = request;
        todo!()
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
    /// [`OpaqueReason::UntypedLayer`](kynos_openapi::OpaqueReason::UntypedLayer)
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

impl<C, P: crate::middleware::catch_panic::PanicPolicy> Router<C, P> {
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
    /// [`OpaqueReason::UntypedLayer`](kynos_openapi::OpaqueReason::UntypedLayer),
    /// and nothing outside it is.
    #[must_use]
    pub fn layer_unchecked<L>(self, layer: L) -> Self
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
        let _ = layer;
        todo!()
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
    #[must_use]
    pub fn route_unchecked<I, H>(self, methods: I, pattern: &'static str, handler: H) -> Self
    where
        C: Sync + 'static,
        I: IntoIterator<Item = crate::http::Method>,
        H: UncheckedHandler<C>,
    {
        let _ = (methods.into_iter().collect::<Vec<_>>(), pattern, handler);
        todo!()
    }

    /// Adds a route that upgrades the connection away from HTTP.
    ///
    /// WebSockets, chiefly. This is not a temporary gap: OpenAPI describes HTTP
    /// request/response semantics, and a socket that stops being either is
    /// outside what any version of the specification can express. `AsyncAPI`
    /// covers this ground, and Kynos would rather point at it than pretend.
    #[must_use]
    pub fn upgrade_unchecked<H>(self, path: &'static str, handler: H) -> Self
    where
        C: Sync + 'static,
        H: UncheckedHandler<C>,
    {
        let _ = (path, handler);
        todo!()
    }

    /// Whether anything unchecked has been added.
    ///
    /// When true, the emitted document carries
    /// `x-kynos-document-not-authoritative`.
    #[must_use]
    pub fn has_unchecked(&self) -> bool {
        todo!()
    }
}

impl<C, P: crate::middleware::catch_panic::PanicPolicy> crate::router::group::Group<C, P> {
    /// Wraps this group in an arbitrary `tower` layer.
    ///
    /// Flags exactly this group's operations, and nothing else. One unchecked
    /// layer on one subtree must not taint three hundred operations it never
    /// touches.
    #[must_use]
    pub fn layer_unchecked<L>(self, layer: L) -> Self
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
        let _ = layer;
        todo!()
    }
}
