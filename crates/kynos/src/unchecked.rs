//! Escape hatches, and what they cost.
//!
//! Everything in this module lets you build something Kynos cannot describe. In
//! exchange, the emitted document is stamped with
//! `x-kynos-document-not-authoritative`, `Router::validate` reports every
//! unchecked construct, and whatever they reach is absent from `paths`.
//!
//! That cost is deliberate and visible. A description that silently omits part
//! of the service is worse than no description, because consumers trust it. If
//! Kynos cannot describe something, it says so in the artifact rather than
//! quietly leaving a hole.
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

use crate::router::Router;

/// A route Kynos does not describe.
#[derive(Debug)]
pub struct UncheckedRoute {
    _private: (),
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
