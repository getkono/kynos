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
//! [`Unchecked`](crate::schema::Unchecked) before reaching for this module,
//! because a weak schema is still an honest one.

use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crate::router::{Router, service::Service};

/// A route Kynos does not describe.
#[derive(Debug)]
pub struct UncheckedRoute {
    _private: (),
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
    /// Returns the document, stamped as non-authoritative.
    #[must_use]
    pub fn openapi(&self) -> &kynos_openapi::Document {
        self.service.openapi()
    }
}

impl<C> Service<C> {
    /// Converts this service into an explicitly unchecked Tower service.
    ///
    /// ```no_run
    /// # use kynos::{router::service::Service, unchecked::UncheckedService};
    /// fn tower<C: Send + Sync + 'static>(service: Service<C>) -> UncheckedService<C> {
    ///     service.into_tower_unchecked()
    /// }
    /// ```
    #[must_use]
    pub fn into_tower_unchecked(self) -> UncheckedService<C> {
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

impl<C> Router<C> {
    /// Wraps the router in an arbitrary `tower` layer.
    ///
    /// A `Layer` may change the status, rewrite the body, add headers, or
    /// refuse the request, and nothing in its type says which — so every
    /// operation underneath becomes a claim Kynos can no longer stand behind.
    ///
    /// Prefer writing an [`Interceptor`](crate::middleware::Interceptor). It is
    /// barely more work: declare an
    /// [`OperationContribution`](crate::middleware::OperationContribution)
    /// saying what the layer does to the exchange, and in return every covered
    /// operation documents it correctly and automatically.
    #[must_use]
    pub fn layer_unchecked<L>(self, layer: L) -> Self
    where
        L: tower::Layer<()> + Send + Sync + 'static,
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
    #[must_use]
    pub fn route_unchecked<H>(self, pattern: &'static str, handler: H) -> Self
    where
        H: Send + Sync + 'static,
    {
        let _ = (pattern, handler);
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
        H: Send + Sync + 'static,
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
