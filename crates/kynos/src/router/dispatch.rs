//! The runtime half of a built router: the match table, and what one request
//! does to it.
//!
//! Private, and named by no path a user can write. Everything here is machinery
//! [`Router::build`](crate::Router::build) assembles and
//! [`Service`](crate::router::service::Service) drives, so there is no item for
//! a canonical path to point at.
//!
//! `matchit` is named here and in [`super`], which is the allowance
//! `docs/architecture.md` gives it.

use std::{
    future::Future, panic::AssertUnwindSafe, pin::Pin, sync::Arc, task::Poll, time::Instant,
};

use kynos_openapi::Method;

use crate::{
    error::problem::{Problem, problem_response},
    extract::params::path::PathCaptures,
    http::{
        HeaderValue, Request, Response, StatusCode,
        body::{Body, Delivery},
        header,
    },
    middleware::{
        Next, Observer,
        erased::{ErasedInterceptor, ErasedTerminal},
    },
    response::IntoResponse,
    router::{
        endpoint::DynEndpoint,
        operation::Route,
        policy::{FallbackPolicy, TrailingSlashPolicy},
    },
    schema::registry::Registry,
};

/// Runs `future` with a panic recovery branch installed.
///
/// No `unsafe`, and no runtime is named: the future is pinned on the heap so
/// that `Pin::as_mut` supplies the projection, and each poll is wrapped in
/// [`catch_unwind`](std::panic::catch_unwind). A future that unwound is
/// reported once and then dropped, never polled again.
pub(crate) async fn recover<F>(future: F) -> Result<Response, Box<dyn std::any::Any + Send>>
where
    F: Future<Output = Response>,
{
    let mut future = Box::pin(future);

    std::future::poll_fn(move |context| {
        match std::panic::catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(context))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(response)) => Poll::Ready(Ok(response)),
            Err(payload) => Poll::Ready(Err(payload)),
        }
    })
    .await
}

/// The response a recovered panic becomes.
///
/// Deliberately says nothing about what panicked: the payload is a message the
/// service's author wrote for themselves, and a client is not its audience.
pub(crate) fn panic_response() -> Response {
    Problem::new(StatusCode::INTERNAL_SERVER_ERROR).into_response()
}

/// The 500 a recovery branch contributes to every operation it covers.
pub(crate) fn panic_responses(registry: &mut Registry) -> kynos_openapi::Responses {
    kynos_openapi::Responses::new().with(
        500,
        problem_response(
            registry,
            "the operation failed unexpectedly and was recovered",
        ),
    )
}

/// An endpoint, as the end of an interceptor chain.
pub(crate) struct EndpointTerminal<C> {
    endpoint: Arc<dyn DynEndpoint<C>>,
}

impl<C> EndpointTerminal<C> {
    pub(crate) fn new(endpoint: Arc<dyn DynEndpoint<C>>) -> Self {
        Self { endpoint }
    }
}

impl<C: Send + Sync + 'static> ErasedTerminal<C> for EndpointTerminal<C> {
    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        self.endpoint.call(request, context)
    }
}

/// A CORS preflight, as the end of a chain that has no interceptors in it.
///
/// Registered on a path while the service is built, after `describe` has
/// finished — which is what makes it out-of-document by construction rather
/// than by a filter someone has to remember.
pub(crate) struct PreflightTerminal {
    preflight: crate::middleware::cors::preflight::Preflight,
}

impl PreflightTerminal {
    pub(crate) fn new(preflight: crate::middleware::cors::preflight::Preflight) -> Self {
        Self { preflight }
    }
}

impl<C: Send + Sync + 'static> ErasedTerminal<C> for PreflightTerminal {
    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        let _ = context;
        Box::pin(async move { self.preflight.answer(&request) })
    }
}

/// One declared operation, ready to serve.
pub(crate) struct Served<C> {
    pub(crate) method: Method,
    pub(crate) operation_id: String,
    /// Erased because an unchecked route ends in a handler that is not an
    /// endpoint, and shared because one such handler serves several methods.
    pub(crate) terminal: Arc<dyn ErasedTerminal<C>>,
    /// Router- and group-scoped interceptors, outermost first. Endpoint-scoped
    /// ones stay inside the endpoint, which is what runs them.
    pub(crate) interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,
    pub(crate) catch_panics: bool,
    /// Layers of undeclared effect covering this operation, outermost first.
    /// Empty for every operation no waiver reached, which is the usual case.
    #[cfg(feature = "unchecked")]
    pub(crate) unchecked_layers: Vec<Arc<dyn crate::unchecked::ErasedLayer>>,
}

/// Every operation declared on one `paths` key.
pub(crate) struct PathEntry<C> {
    /// The `paths` key, exactly as the description spells it.
    pub(crate) template: String,
    /// The same key, interned so that
    /// [`MatchedPath`](crate::extract::connection::MatchedPath) can hold it.
    ///
    /// That extractor is infallible and reads the template back out of the
    /// request extensions, so the value has to outlive the request and cannot
    /// borrow `template`. Interned once per
    /// [`Router::build`](crate::Router::build), like the variable names below.
    pub(crate) matched: crate::extract::connection::MatchedPath,
    /// The template's variable names, in declaration order.
    ///
    /// `&'static str` because [`PathCaptures`] stores them, so that a capture
    /// costs one allocation for the vector and none per variable. The names are
    /// interned once per [`Router::build`](crate::Router::build) — a set
    /// bounded by the route table, which a program builds at startup.
    pub(crate) variables: Vec<&'static str>,
    /// The `Allow` header a 405 on this path carries, derived from the
    /// operations below rather than restated beside them.
    pub(crate) allow: HeaderValue,
    pub(crate) operations: Vec<Served<C>>,
}

impl<C> PathEntry<C> {
    /// Where this path declares `method`, if it does.
    ///
    /// A position rather than a reference, because an operation wrapped in an
    /// unchecked layer is re-entered by index once the layer calls through.
    fn position(&self, method: Method) -> Option<usize> {
        self.operations
            .iter()
            .position(|operation| operation.method == method)
    }
}

/// The whole route table, plus everything a request needs that is not a route.
pub(crate) struct Dispatch<C> {
    pub(crate) matcher: matchit::Router<usize>,
    pub(crate) paths: Vec<PathEntry<C>>,
    pub(crate) context: C,
    pub(crate) observers: Vec<Arc<dyn Observer<C>>>,
    pub(crate) not_found: FallbackPolicy,
    pub(crate) method_not_allowed: FallbackPolicy,
    pub(crate) trailing_slashes: TrailingSlashPolicy,
    pub(crate) trusted_proxies: crate::http::forwarded::TrustedProxies,
}

/// Where in the table an operation sits.
///
/// Indices rather than a [`Route`], because a route borrows the table and the
/// response body outlives every such borrow: the driver holds it after
/// [`serve`](Dispatch::serve) has returned.
#[derive(Clone, Copy, Debug)]
struct Location {
    path: usize,
    position: usize,
}

impl<C: Send + Sync + 'static> Dispatch<C> {
    /// Serves one request.
    ///
    /// Takes the handle rather than a borrow of it so that an unchecked layer,
    /// whose future has no lifetime to borrow through, can be handed a
    /// continuation that re-enters the table.
    pub(crate) async fn serve(self: Arc<Self>, mut request: Request) -> Response {
        let started = Instant::now();

        // The captures are taken here, while the match still holds them, and
        // are ranges rather than borrows -- which is what lets the request be
        // mutated below without re-matching.
        let (index, captures) = {
            let path = request.uri().path();
            let Ok(matched) = self.matcher.at(path) else {
                let response = self.unmatched(&request);
                return self.finish(response, None, started);
            };

            let index = *matched.value;
            let variables = &self.paths[index].variables;
            let captures = (!variables.is_empty()).then(|| {
                PathCaptures::new(
                    path,
                    variables
                        .iter()
                        .filter_map(|name| matched.params.get(name).map(|value| (*name, value))),
                )
            });

            (index, captures)
        };

        let entry = &self.paths[index];
        let position = Method::from_wire_str(request.method().as_str())
            .and_then(|method| entry.position(method));

        let Some(position) = position else {
            let response = fallback(StatusCode::METHOD_NOT_ALLOWED, &self.method_not_allowed);
            let response = with_allow(response, &entry.allow);
            return self.finish(response, None, started);
        };

        let operation = &entry.operations[position];
        let at = Location {
            path: index,
            position,
        };
        let route = Route::new(&entry.template, &operation.operation_id, operation.method);

        if let Some(captures) = captures {
            request.extensions_mut().insert(captures);
        }

        // The template rather than the request's own path: `MatchedPath` is
        // documented as the `paths` key, which is what keeps a metric label or
        // a log field from having unbounded cardinality.
        request.extensions_mut().insert(entry.matched.clone());

        // Resolved once, here, rather than by each reader. Two interceptors
        // parsing `Forwarded` for themselves would be two answers to one
        // security question, and the policy that governs it is the router's.
        let peer = request
            .extensions()
            .get::<crate::extract::connection::Connection>()
            .filter(|connection| !connection.is_in_process())
            .map(crate::extract::connection::Connection::peer_addr);
        let forwarded = crate::http::forwarded::Forwarded::resolve(
            request.headers(),
            peer,
            &self.trusted_proxies,
        );
        request.extensions_mut().insert(forwarded);

        for observer in &self.observers {
            observer.on_request(&request, Some(route), &self.context);
        }

        // A layer is outside the description, so it wraps the operation from
        // outside too -- after routing, exactly where an interceptor runs.
        #[cfg(feature = "unchecked")]
        if !operation.unchecked_layers.is_empty() {
            let response = crate::unchecked::through_layers(
                &operation.unchecked_layers,
                Arc::clone(&self),
                index,
                position,
                request,
            )
            .await;
            return self.finish(response, Some(at), started);
        }

        let response = self.run(operation, route, request).await;
        self.finish(response, Some(at), started)
    }

    /// Runs one already-routed operation's chain, with recovery if it asked for
    /// it.
    async fn run(&self, operation: &Served<C>, route: Route<'_>, request: Request) -> Response {
        let served = Next::new(
            &operation.interceptors,
            &*operation.terminal,
            &self.context,
            route,
        )
        .run(request);

        if operation.catch_panics {
            match recover(async move { served.await.into_response() }).await {
                Ok(response) => response,
                Err(payload) => {
                    for observer in &self.observers {
                        observer.on_panic(payload.as_ref(), Some(route));
                    }
                    panic_response()
                }
            }
        } else {
            served.await.into_response()
        }
    }

    /// Re-enters the table at an operation an unchecked layer has called
    /// through to.
    ///
    /// By index because the continuation a layer carries outlives every borrow
    /// of the table -- a `tower` service's future has no lifetime parameter.
    #[cfg(feature = "unchecked")]
    pub(crate) fn resume(
        self: Arc<Self>,
        path: usize,
        position: usize,
        request: Request,
    ) -> Pin<Box<dyn Future<Output = Response> + Send>> {
        Box::pin(async move {
            let entry = &self.paths[path];
            let operation = &entry.operations[position];
            let route = Route::new(&entry.template, &operation.operation_id, operation.method);
            self.run(operation, route, request).await
        })
    }

    /// Names the operation at `at`, borrowing the table's own strings.
    fn route_at(&self, at: Location) -> Route<'_> {
        let entry = &self.paths[at.path];
        let operation = &entry.operations[at.position];

        Route::new(&entry.template, &operation.operation_id, operation.method)
    }

    /// Notifies every observer and hands the response on.
    ///
    /// The response leaves here wearing a watch on its body, so that a peer
    /// that goes away mid-response is reported rather than silently counted as
    /// served. Only when there is an observer to tell: a router with none pays
    /// nothing, which keeps the watch off the path of every service that never
    /// asked to observe anything.
    fn finish(
        self: &Arc<Self>,
        response: Response,
        at: Option<Location>,
        started: Instant,
    ) -> Response {
        if self.observers.is_empty() {
            return response;
        }

        let elapsed = started.elapsed();
        let route = at.map(|at| self.route_at(at));
        for observer in &self.observers {
            observer.on_response(&response, route, elapsed);
        }

        // The route is rebuilt inside the watch rather than captured: it
        // borrows the table, and the body outlives every borrow taken here --
        // it is handed to the protocol driver and dropped whenever that driver
        // is done with it.
        let watcher = Arc::clone(self);
        let (parts, body) = response.into_parts();
        let body = body.watching(move |delivery| {
            if delivery == Delivery::Complete {
                return;
            }

            let elapsed = started.elapsed();
            let route = at.map(|at| watcher.route_at(at));
            for observer in &watcher.observers {
                observer.on_disconnect(route, elapsed);
            }
        });

        Response::from_parts(parts, body)
    }

    /// What a request that matched no route gets.
    ///
    /// Under [`TrailingSlashPolicy::Redirect`] a path that reaches an exactly
    /// declared one by adding or removing its final slash is redirected there
    /// with 308, so the method and the body survive the replay. Nothing else
    /// about the path is touched: no casing, no normalization, and no per-route
    /// exception.
    fn unmatched(&self, request: &Request) -> Response {
        if self.trailing_slashes == TrailingSlashPolicy::Redirect {
            if let Some(target) = self.flipped(request.uri().path()) {
                return redirect(&target, request.uri().query());
            }
        }

        fallback(StatusCode::NOT_FOUND, &self.not_found)
    }

    /// The same path with its final slash added or removed, when that reaches a
    /// declared route.
    fn flipped(&self, path: &str) -> Option<String> {
        let candidate = flip_trailing_slash(path)?;

        self.matcher.at(&candidate).is_ok().then_some(candidate)
    }
}

/// The same path with its final slash added or removed.
///
/// `None` for `/`, which has no shorter form: stripping its slash would leave
/// no path at all.
///
/// Shared deliberately. [`TrailingSlashPolicy::Redirect`] flips a request
/// target here at request time and [`TrailingSlashPolicy::Lenient`] flips a
/// declared template at build time, and the two policies would be incoherent if
/// they disagreed about what the other spelling of a path is.
pub(crate) fn flip_trailing_slash(path: &str) -> Option<String> {
    match path.strip_suffix('/') {
        Some("") => None,
        Some(shorter) => Some(shorter.to_owned()),
        None => Some(format!("{path}/")),
    }
}

/// The body shape a fallback takes, which is all a [`FallbackPolicy`]
/// chooses — never the status.
fn fallback(status: StatusCode, policy: &FallbackPolicy) -> Response {
    match policy {
        FallbackPolicy::Problem => Problem::new(status).into_response(),
        FallbackPolicy::Empty => {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = status;
            response
        }
    }
}

/// Attaches the `Allow` header RFC 9110 requires on a 405.
fn with_allow(mut response: Response, allow: &HeaderValue) -> Response {
    response.headers_mut().insert(header::ALLOW, allow.clone());
    response
}

/// The 308 a trailing-slash redirect answers with.
fn redirect(path: &str, query: Option<&str>) -> Response {
    let target = match query {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    };

    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::PERMANENT_REDIRECT;
    if let Ok(location) = HeaderValue::from_str(&target) {
        response.headers_mut().insert(header::LOCATION, location);
    }

    response
}

/// The `Allow` header value for a set of declared methods.
///
/// Derived from the operations actually declared, which is what stops it
/// disagreeing with the description.
pub(crate) fn allow_header(methods: &[Method]) -> HeaderValue {
    let joined = methods
        .iter()
        .map(|method| method.as_wire_str())
        .collect::<Vec<_>>()
        .join(", ");

    HeaderValue::from_str(&joined).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Interns a path variable name for the life of the process.
///
/// [`PathCaptures`] stores names as `&'static str` so that a capture borrows
/// the request path rather than owning a copy of it. Nothing shorter-lived can
/// satisfy that, and the set is bounded by the route table, so the router
/// interns each name once while it is built.
pub(crate) fn intern(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}
