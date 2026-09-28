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
    any::Any,
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    task::Poll,
    time::Instant,
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
pub(crate) async fn recover<F>(future: F) -> Result<Response, Box<dyn Any + Send>>
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

/// The payload of a panic an endpoint recovered, on its way to the dispatcher.
///
/// Carried on the 500's extensions because `Endpoint::call` has no other way
/// out, and reported where the route and the observers already are. Behind a
/// lock because an extension must be `Clone + Sync` and a payload is only
/// `Send`. Private, so nothing between the endpoint and the dispatcher can
/// name it.
#[derive(Clone)]
struct Recovered(Arc<Mutex<Option<Box<dyn Any + Send>>>>);

/// [`panic_response`], carrying the payload it was recovered from.
pub(crate) fn recovered_response(payload: Box<dyn Any + Send>) -> Response {
    let mut response = panic_response();
    response
        .extensions_mut()
        .insert(Recovered(Arc::new(Mutex::new(Some(payload)))));
    response
}

/// Removes the payload [`recovered_response`] attached, if this is one.
fn take_recovered(response: &mut Response) -> Option<Box<dyn Any + Send>> {
    response
        .extensions_mut()
        .remove::<Recovered>()?
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
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

/// What routing learned about one request, for whatever reads it afterwards.
///
/// Inserted once per matched request, and the one extension routing adds; an
/// unchecked layer adds its own continuation, and only where one is mounted.
/// Each insertion into [`http::Extensions`](crate::http::Extensions) boxes its
/// value, so three facts inserted separately cost three allocations; carried
/// together they cost one. Every field is filled at the same point in
/// [`Dispatch::serve`] the separate insertions ran at, so each reader sees
/// exactly what it saw before. Inserting a `MatchedPath` or `Forwarded` into
/// the extensions, by contrast, no longer has any effect: every reader goes
/// through this record, and code holding a `&Request` borrows the origin with
/// [`Forwarded::of`](crate::http::forwarded::Forwarded::of).
///
/// Read through the extractors and keys that expose each fact —
/// [`MatchedPath`](crate::extract::connection::MatchedPath),
/// [`Path`](crate::extract::params::path::Path),
/// [`Forwarded`](crate::http::forwarded::Forwarded),
/// [`captured`](crate::unchecked::captured) and
/// [`ByClientAddress`](crate::middleware::rate_limit::key::ByClientAddress) —
/// never by name outside the crate.
#[derive(Clone, Debug)]
pub(crate) struct Routed {
    /// The `paths` key that matched.
    pub(crate) matched: crate::extract::connection::MatchedPath,
    /// What the match captured, when the template has variables.
    pub(crate) captures: Option<PathCaptures>,
    /// Where the request came from, resolved under the router's trust policy
    /// before any interceptor runs.
    ///
    /// Resolved once, by the dispatcher, rather than by each reader. Two
    /// interceptors parsing `Forwarded` for themselves would be two answers to
    /// one security question, and the policy that governs it is the router's.
    pub(crate) forwarded: crate::http::forwarded::Forwarded,
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
    /// operations below rather than restated beside them: their methods, and
    /// `HEAD` wherever `GET` is one of them.
    pub(crate) allow: HeaderValue,
    pub(crate) operations: Vec<Served<C>>,
}

impl<C> PathEntry<C> {
    /// Where the operation answering `method` sits, if one does.
    ///
    /// The one declaring `method`, and for a `HEAD` no operation declares, the
    /// `GET`: RFC 9110 section 9.3.2 defines a HEAD as that GET without
    /// content, so the GET operation describes it. A declared `head` wins.
    ///
    /// A position rather than a reference, because an operation wrapped in an
    /// unchecked layer is re-entered by index once the layer calls through.
    fn position(&self, method: Method) -> Option<usize> {
        let declared = |method| {
            self.operations
                .iter()
                .position(|operation| operation.method == method)
        };

        declared(method).or_else(|| {
            if method == Method::Head {
                declared(Method::Get)
            } else {
                None
            }
        })
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
    /// Every method some operation in the service answers, which is what
    /// tells a 405 from a 501. See [`implemented`].
    pub(crate) implemented: Vec<Method>,
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
        // Whatever answers a HEAD -- an operation, a fallback, a redirect --
        // sends no content, so this is read before anything can answer.
        let head = request.method() == crate::http::Method::HEAD;
        let method = Method::from_wire_str(request.method().as_str());

        // The captures are taken here, while the match still holds them, and
        // are ranges rather than borrows -- which is what lets the request be
        // mutated below without re-matching.
        let (index, captures) = {
            let path = request.uri().path();
            let Ok(matched) = self.matcher.at(path) else {
                // RFC 9110 section 15.6.2: a 501 is about the server, not the
                // resource, so no 404 or redirect is owed first.
                let response = if self.implements(method) {
                    self.unmatched(&request)
                } else {
                    method_refusal(None, &self.method_not_allowed)
                };
                return self.finish(response, None, started, head);
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

        let Some(position) = method.and_then(|method| entry.position(method)) else {
            let allow = self.implements(method).then_some(&entry.allow);
            let response = method_refusal(allow, &self.method_not_allowed);
            return self.finish(response, None, started, head);
        };

        let operation = &entry.operations[position];
        let at = Location {
            path: index,
            position,
        };
        let route = Route::new(&entry.template, &operation.operation_id, operation.method);

        let forwarded = self.forwarded(&request);
        request.extensions_mut().insert(Routed {
            // The template rather than the request's own path: `MatchedPath`
            // is documented as the `paths` key, which is what keeps a metric
            // label or a log field from having unbounded cardinality.
            matched: entry.matched.clone(),
            captures,
            forwarded,
        });

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
            return self.finish(response, Some(at), started, head);
        }

        let response = self.run(operation, route, request).await;
        self.finish(response, Some(at), started, head)
    }

    /// Whether some operation in the service answers `method`.
    ///
    /// `None` is a token no description can declare, which nothing answers.
    fn implements(&self, method: Option<Method>) -> bool {
        method.is_some_and(|method| self.implemented.contains(&method))
    }

    /// Where `request` came from, as far as the trusted proxies say.
    fn forwarded(&self, request: &Request) -> crate::http::forwarded::Forwarded {
        let peer = request
            .extensions()
            .get::<crate::extract::connection::Connection>()
            .filter(|connection| !connection.is_in_process())
            .map(crate::extract::connection::Connection::peer_addr);

        crate::http::forwarded::Forwarded::resolve(request.headers(), peer, &self.trusted_proxies)
    }

    /// Runs one already-routed operation's chain, with recovery if it asked for
    /// it.
    ///
    /// A panic is reported here whichever scope recovered it: this one, or the
    /// endpoint's own, whose 500 carries the payload out.
    async fn run(&self, operation: &Served<C>, route: Route<'_>, request: Request) -> Response {
        let served = Next::new(
            &operation.interceptors,
            &*operation.terminal,
            &self.context,
            route,
        )
        .run(request);

        let mut response = if operation.catch_panics {
            match recover(async move { served.await.into_response() }).await {
                Ok(response) => response,
                Err(payload) => {
                    self.report_panic(payload.as_ref(), route);
                    panic_response()
                }
            }
        } else {
            served.await.into_response()
        };

        // Taken whether or not anyone observes, so the marker never reaches the
        // driver.
        if let Some(payload) = take_recovered(&mut response) {
            self.report_panic(payload.as_ref(), route);
        }
        response
    }

    /// Tells every observer about a recovered panic.
    fn report_panic(&self, payload: &(dyn Any + Send), route: Route<'_>) {
        for observer in &self.observers {
            observer.on_panic(payload, Some(route));
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
    ///
    /// A response to a HEAD sheds its content first, so an observer sees what
    /// the peer will.
    fn finish(
        self: &Arc<Self>,
        response: Response,
        at: Option<Location>,
        started: Instant,
        head: bool,
    ) -> Response {
        let response = if head {
            without_content(response)
        } else {
            response
        };

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

/// What a request whose method no operation on its path answers gets.
///
/// RFC 9110 section 9.1 splits the two cases. With `allow`, the method is one
/// the service implements elsewhere: a 405 carrying the path's `Allow`, which
/// section 15.5.6 requires on one. Without, nothing implements it: a 501, with
/// no `Allow` to offer. Either takes the router's method-not-allowed policy's
/// shape, and the CORS preflight answers a plain `OPTIONS` through here too, so
/// mounting CORS changes neither.
pub(crate) fn method_refusal(allow: Option<&HeaderValue>, policy: &FallbackPolicy) -> Response {
    let Some(allow) = allow else {
        return fallback(StatusCode::NOT_IMPLEMENTED, policy);
    };

    let mut response = fallback(StatusCode::METHOD_NOT_ALLOWED, policy);
    response.headers_mut().insert(header::ALLOW, allow.clone());
    response
}

/// `response` as the answer to a HEAD: the same status and fields, and no
/// content.
///
/// RFC 9110 section 9.3.2: the server "MUST NOT send content" in response to a
/// HEAD. HTTP/1.1 would drop the body on the wire, but hyper's HTTP/2 server
/// sends whatever body it is handed, so it is dropped here.
///
/// `Content-Length` is stated first where the body knows a non-zero length:
/// section 8.6 lets a HEAD carry the length the GET would have sent and forbids
/// any other. An empty body is no evidence of an empty GET -- a declared `head`
/// answers with none -- so a zero is never stated, the rule hyper's HTTP/1.1
/// encoder keeps. A length the response already carries is left alone, and a
/// status that never carries content gets none.
fn without_content(response: Response) -> Response {
    use http_body::Body as _;

    let (mut parts, body) = response.into_parts();

    let bodiless = parts.status.is_informational()
        || parts.status == StatusCode::NO_CONTENT
        || parts.status == StatusCode::NOT_MODIFIED;
    if !bodiless && !parts.headers.contains_key(header::CONTENT_LENGTH) {
        if let Some(length) = body.size_hint().exact().filter(|&length| length != 0) {
            parts
                .headers
                .insert(header::CONTENT_LENGTH, HeaderValue::from(length));
        }
    }

    Response::from_parts(parts, Body::empty())
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
/// disagreeing with the description, plus the `HEAD` a declared `GET` answers
/// where no `head` is declared, named right after it.
pub(crate) fn allow_header(methods: &[Method]) -> HeaderValue {
    let derives_head = !methods.contains(&Method::Head);
    let joined = methods
        .iter()
        .flat_map(|method| {
            let head = (derives_head && *method == Method::Get).then_some(Method::Head);
            std::iter::once(*method).chain(head)
        })
        .map(Method::as_wire_str)
        .collect::<Vec<_>>()
        .join(", ");

    HeaderValue::from_str(&joined).unwrap_or_else(|_| HeaderValue::from_static(""))
}

/// Every method some operation in `paths` answers: each declared one, and
/// `HEAD` wherever a `GET` is declared.
///
/// Read before the CORS preflights are installed, so the `OPTIONS` one answers
/// is not counted: a preflight is not an operation, and counting it would turn
/// a plain `OPTIONS` from a 501 into a 405 the moment CORS was mounted.
pub(crate) fn implemented<C>(paths: &[PathEntry<C>]) -> Vec<Method> {
    let mut methods: Vec<Method> = Vec::new();

    for operation in paths.iter().flat_map(|entry| &entry.operations) {
        let derived = (operation.method == Method::Get).then_some(Method::Head);
        for method in std::iter::once(operation.method).chain(derived) {
            if !methods.contains(&method) {
                methods.push(method);
            }
        }
    }

    methods
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

#[cfg(test)]
mod tests;
