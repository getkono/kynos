//! The runtime half of a built router: the match table, and what one request
//! does to it.
//!
//! `matchit` may be named here and in [`super`] (`docs/architecture.md`).

pub(crate) mod recovery;

use std::{any::Any, future::Future, pin::Pin, sync::Arc, time::Instant};

use kynos_openapi::Method;

use crate::{
    error::problem::Problem,
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
        dispatch::recovery::{panic_response, recover, take_recovered},
        endpoint::DynEndpoint,
        operation::Route,
        policy::{FallbackPolicy, TrailingSlashPolicy},
    },
};

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
/// Registered after `describe` has finished, so it is out-of-document by
/// construction.
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
    /// Whether the described operation declares a security requirement; false
    /// for an unchecked route and a synthesized preflight.
    pub(crate) secured: bool,
    /// Layers of undeclared effect covering this operation, outermost first.
    #[cfg(feature = "unchecked")]
    pub(crate) unchecked_layers: Vec<Arc<dyn crate::unchecked::ErasedLayer>>,
}

/// What routing learned about one request, for whatever reads it afterwards.
///
/// One extension rather than several, since each insertion allocates. Read
/// only through the extractors and keys that expose each fact.
#[derive(Clone, Debug)]
pub(crate) struct Routed {
    /// The `paths` key that matched.
    pub(crate) matched: crate::extract::connection::MatchedPath,
    /// What the match captured, when the template has variables.
    pub(crate) captures: Option<PathCaptures>,
    /// Where the request came from, resolved once under the router's trust
    /// policy before any interceptor runs, so every reader gets one answer.
    pub(crate) forwarded: crate::http::forwarded::Forwarded,
    /// Whether the matched operation declares a security requirement, true
    /// also for one admitting anonymous access beside it. Read by `Cache`,
    /// whose hit is served before the operation's guard runs.
    #[cfg_attr(
        not(feature = "cache"),
        expect(dead_code, reason = "the cache is the one reader")
    )]
    pub(crate) secured: bool,
}

/// Every operation declared on one `paths` key.
pub(crate) struct PathEntry<C> {
    /// The `paths` key, exactly as the description spells it.
    pub(crate) template: String,
    /// The same key, interned so that
    /// [`MatchedPath`](crate::extract::connection::MatchedPath) can hold it.
    pub(crate) matched: crate::extract::connection::MatchedPath,
    /// The template's variable names, in declaration order, interned because
    /// [`PathCaptures`] stores `&'static str`.
    pub(crate) variables: Vec<&'static str>,
    /// The `Allow` header a 405 on this path carries, derived from the
    /// operations below plus `HEAD` wherever `GET` is one of them.
    pub(crate) allow: HeaderValue,
    pub(crate) operations: Vec<Served<C>>,
}

impl<C> PathEntry<C> {
    /// Where the operation answering `method` sits, if one does.
    ///
    /// An undeclared `HEAD` falls back to the `GET` (RFC 9110 section 9.3.2).
    /// A position, because an unchecked layer re-enters the table by index.
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

/// Where in the table an operation sits; indices because the response body
/// outlives any borrow of the table.
#[derive(Clone, Copy, Debug)]
struct Location {
    path: usize,
    position: usize,
}

impl<C: Send + Sync + 'static> Dispatch<C> {
    /// Serves one request.
    ///
    /// Takes the `Arc` so an unchecked layer can be handed a continuation that
    /// re-enters the table.
    pub(crate) async fn serve(self: Arc<Self>, mut request: Request) -> Response {
        let started = Instant::now();
        // Whatever answers a HEAD sends no content.
        let head = request.method() == crate::http::Method::HEAD;
        let method = Method::from_wire_str(request.method().as_str());

        // Captures are ranges rather than borrows, so the request stays mutable.
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
            // The template, not the request path, to bound label cardinality.
            matched: entry.matched.clone(),
            captures,
            forwarded,
            secured: operation.secured,
        });

        for observer in &self.observers {
            observer.on_request(&request, Some(route), &self.context);
        }

        // A layer wraps the operation after routing, where an interceptor runs.
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
    /// through to, by index since a `tower` future cannot borrow the table.
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
    /// With observers, the body is watched so a peer leaving mid-response is
    /// reported; without, nothing is added. A HEAD response sheds its content
    /// first, so an observer sees what the peer will.
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

        // Rebuilt inside the watch: the body outlives any borrow of the table.
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
    /// Under [`TrailingSlashPolicy::Redirect`], a path one final slash away
    /// from a declared one gets a 308 there; otherwise the 404 fallback.
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
/// `None` for `/`. Shared by `Redirect` (request time) and `Lenient` (build
/// time) so the two policies agree on a path's other spelling.
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
/// With `allow`, a 405 carrying it (RFC 9110 section 15.5.6); without, a 501,
/// since nothing in the service implements the method (section 9.1). Also the
/// CORS preflight's answer to a plain `OPTIONS`.
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
/// RFC 9110 section 9.3.2 forbids content, and hyper's HTTP/2 server would
/// send it. A known non-zero length becomes `Content-Length` (section 8.6); a
/// zero is never stated, since an empty body is no evidence of an empty GET.
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
/// Plus the `HEAD` a declared `GET` answers where no `head` is declared, named
/// right after it.
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
/// Read before CORS preflights are installed: counting their `OPTIONS` would
/// turn a plain `OPTIONS` 501 into a 405 once CORS is mounted.
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

/// Interns a path variable name for the life of the process. Leaks once per
/// name per build, a set bounded by the route table.
pub(crate) fn intern(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

#[cfg(test)]
mod tests;
