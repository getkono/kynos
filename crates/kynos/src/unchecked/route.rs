//! Taking a waiver: registering an unchecked route or layer, and recording it.
//!
//! Separate from [`super`], which bridges `tower` into the request path,
//! because what a waiver records changes independently of how a request runs
//! through one.

use std::{convert::Infallible, marker::PhantomData, sync::Arc};

use kynos_openapi::{Method, OpaqueReason, OpaqueRoute};

use crate::{
    middleware::{catch_panic::PanicPolicy, erased::ErasedInterceptor},
    router::{Router, group::Group},
    unchecked::{
        Unchecked, UncheckedHandler, UncheckedInner, UncheckedRoute, UncheckedTerminal, erase,
    },
};

impl<C> UncheckedRoute<C> {
    /// A route served by `handler`, under no enclosing scope yet.
    fn new<H>(pattern: String, methods: Vec<Method>, handler: H, record: OpaqueRoute) -> Self
    where
        C: Sync + 'static,
        H: UncheckedHandler<C>,
    {
        Self {
            pattern,
            methods,
            terminal: Arc::new(UncheckedTerminal {
                handler,
                _context: PhantomData,
            }),
            layers: Vec::new(),
            interceptors: Vec::new(),
            catch_panics: false,
            record,
        }
    }

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

impl<C> Unchecked<C> {
    /// Takes over another router's waivers, under `prefix`.
    ///
    /// The absorbed router's layers, `interceptors` and panic policy become
    /// part of each of its routes, since they covered exactly those.
    pub(crate) fn absorb(
        &mut self,
        other: Self,
        prefix: &str,
        interceptors: &[Arc<dyn ErasedInterceptor<C>>],
        catch_panics: bool,
    ) {
        let Self { routes, layers } = other;

        for mut route in routes {
            route.reprefix(prefix);
            // The enclosing scope's covering, outermost, before the route's own.
            route.layers.splice(0..0, layers.iter().cloned());
            route
                .interceptors
                .splice(0..0, interceptors.iter().cloned());
            route.catch_panics |= catch_panics;
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

    (literal.len() < pattern.split('/').count() && literal.len() > 1).then(|| literal.join("/"))
}

/// The record of a route under `pattern`, with the prefix it is anchored at.
fn anchored_record(pattern: &str, reason: OpaqueReason) -> OpaqueRoute {
    let mut record = OpaqueRoute::new(pattern, reason);
    record.prefix = anchor(pattern);
    record
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
    /// A method and pattern already declared, described or unchecked, is an
    /// error-level `DuplicateOperation`, since only the first would be served.
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

        let mut record = anchored_record(pattern, reason)
            .with_methods(methods.iter().map(|method| method.as_wire_str()));
        if !unmodelled.is_empty() {
            record = record.with_note(format!(
                "not served: {} has no Path Item field, so Kynos will not route it",
                unmodelled.join(", ")
            ));
        }

        self.unchecked.routes.push(UncheckedRoute::new(
            pattern.to_owned(),
            methods,
            handler,
            record,
        ));
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
        self.unchecked.routes.push(UncheckedRoute::new(
            pattern,
            vec![Method::Get],
            handler,
            record,
        ));
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
    /// The handler answers `101 Switching Protocols` and takes the connection
    /// with `hyper::upgrade::on(&mut request)`, awaited on a spawned task since
    /// it resolves only once the 101 is written. Only an HTTP/1.1 connection
    /// can upgrade: HTTP/2 has no `Upgrade` (RFC 9113 §8.6).
    ///
    /// [RFC 9110]: https://www.rfc-editor.org/rfc/rfc9110
    #[must_use]
    pub fn upgrade_unchecked<H>(mut self, path: &str, handler: H) -> Self
    where
        C: Sync + 'static,
        H: UncheckedHandler<C>,
    {
        let record = anchored_record(path, OpaqueReason::ProtocolUpgrade)
            .with_methods([Method::Get.as_wire_str()])
            .with_note("the connection leaves HTTP, which no version of the specification models");

        self.unchecked.routes.push(UncheckedRoute::new(
            path.to_owned(),
            vec![Method::Get],
            handler,
            record,
        ));
        self
    }

    /// Whether anything unchecked has been added.
    ///
    /// When true, the emitted document carries
    /// `x-kynos-document-not-authoritative`.
    ///
    /// It is what a CI job should assert false; [`examples/unchecked.rs`]
    /// shows where the check goes. Reach for
    /// [`unchecked_reasons`](Self::unchecked_reasons) where a service takes one
    /// waiver deliberately.
    ///
    /// [`examples/unchecked.rs`]: https://github.com/getkono/kynos/blob/master/crates/kynos/examples/unchecked.rs
    #[must_use]
    pub fn has_unchecked(&self) -> bool {
        !self.unchecked.routes.is_empty() || self.has_unchecked_layers()
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
        if self.has_unchecked_layers() {
            record(OpaqueReason::UntypedLayer);
        }

        reasons
    }

    /// Whether an unchecked `tower` layer covers this router or any group
    /// mounted on it.
    fn has_unchecked_layers(&self) -> bool {
        !self.unchecked.layers.is_empty()
            || self
                .mounted
                .iter()
                .any(|mounted| !mounted.unchecked_layers.is_empty())
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

#[cfg(test)]
mod tests;
