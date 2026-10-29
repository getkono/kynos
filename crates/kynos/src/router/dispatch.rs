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
    /// The `paths` key that matched; `None` for a fallback, which only the
    /// router's own interceptors see.
    pub(crate) matched: Option<crate::extract::connection::MatchedPath>,
    /// What the match captured, when the template has variables.
    pub(crate) captures: Option<PathCaptures>,
    /// Where the request came from, resolved once under the router's trust
    /// policy before any interceptor runs, so every reader gets one answer.
    pub(crate) forwarded: crate::http::forwarded::Forwarded,
    /// Whether the matched operation declares a security requirement, true
    /// also for one admitting anonymous access beside it, and false where none
    /// matched. Read by `Cache`, whose hit is served before the operation's
    /// guard runs.
    #[cfg_attr(
        not(feature = "cache"),
        expect(dead_code, reason = "the cache is the one reader")
    )]
    pub(crate) secured: bool,
    /// Where each operation is mounted, for a `Link` argument to read; `None`
    /// for a fallback and wherever no operation links, so a service without
    /// links pays nothing for them.
    pub(crate) links: Option<Arc<crate::extract::link::Links>>,
}

/// The `paths` key routing matched `request` to, prefixes included; `None`
/// where no router routed it.
pub(crate) fn mounted_path(request: &Request) -> Option<&'static str> {
    request
        .extensions()
        .get::<Routed>()
        .and_then(|routed| routed.matched.as_ref())
        .map(|matched| matched.0)
}

/// Every operation declared on one `paths` key.
pub(crate) struct PathEntry<C> {
    /// The `paths` key, exactly as the description spells it.
    pub(crate) template: String,
    /// The same key, interned so that
    /// [`MatchedPath`](crate::extract::connection::MatchedPath) can hold it.
    pub(crate) matched: crate::extract::connection::MatchedPath,
    /// The template's variable names, in declaration order, interned with the
    /// list holding them because every [`PathCaptures`] borrows that list.
    pub(crate) variables: &'static [&'static str],
    /// The `Allow` header a 405 on this path carries, derived from the
    /// operations below plus `HEAD` wherever `GET` is one of them.
    pub(crate) allow: HeaderValue,
    pub(crate) operations: Vec<Served<C>>,
}

impl<C> PathEntry<C> {
    /// Sets [`allow`](Self::allow) from the operations declared on this path.
    pub(crate) fn derive_allow(&mut self) {
        let methods: Vec<_> = self
            .operations
            .iter()
            .map(|operation| operation.method)
            .collect();
        self.allow = allow_header(&methods);
    }

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
    /// The root router's own interceptors, outermost first, which also run
    /// around every fallback; a group's or a nested router's cover only their
    /// operations.
    pub(crate) interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,
    /// Whether the root router asked for recovery, which then covers a
    /// fallback's chain too.
    pub(crate) catch_panics: bool,
    pub(crate) not_found: FallbackPolicy,
    pub(crate) method_not_allowed: FallbackPolicy,
    pub(crate) trailing_slashes: TrailingSlashPolicy,
    pub(crate) trusted_proxies: crate::http::forwarded::TrustedProxies,
    /// Every method some operation in the service answers, which is what
    /// tells a 405 from a 501. See [`implemented`].
    pub(crate) implemented: Vec<Method>,
    /// Handed to every routed request; see [`Routed::links`].
    pub(crate) links: Option<Arc<crate::extract::link::Links>>,
}

/// Why no operation answers a request.
#[derive(Clone, Copy, Debug)]
enum Unanswered {
    /// No path matched and the method is implemented somewhere: a 404, or a
    /// 308 under [`TrailingSlashPolicy::Redirect`].
    Path,
    /// No operation answers the method: a 405 carrying the `Allow` of the
    /// path at `path`, or a 501 where `path` is `None`.
    Method { path: Option<usize> },
}

/// A fallback, as the end of the root router's interceptor chain.
///
/// Owns its share of the table, since a terminal is `'static`.
struct FallbackTerminal<C> {
    dispatch: Arc<Dispatch<C>>,
    unanswered: Unanswered,
}

impl<C: Send + Sync + 'static> ErasedTerminal<C> for FallbackTerminal<C> {
    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        let _ = context;
        Box::pin(async move { self.dispatch.answer(self.unanswered, &request) })
    }
}

/// A chain [`Dispatch::chain`] has started, under recovery or not.
enum Started<R, U> {
    /// Boxed by `recover`, so a pointer wide.
    Recovering(R),
    Unguarded(U),
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
    ///
    /// Routes before the returned future exists, and that future awaits each
    /// chain it starts in place rather than through another `async fn`: every
    /// request in flight carries this future in a box, and each `async fn`
    /// between it and the chain would keep a copy of the request of its own.
    pub(crate) fn serve(
        self: Arc<Self>,
        mut request: Request,
    ) -> impl Future<Output = Response> + Send + 'static {
        // Only an observer reads the clock.
        let started = (!self.observers.is_empty()).then(Instant::now);
        // Whatever answers a HEAD sends no content.
        let head = request.method() == crate::http::Method::HEAD;
        let routed = self.route(&mut request);

        async move {
            let response = 'served: {
                let at = match routed {
                    Ok(at) => at,
                    // Without interceptors nothing waits, so a fallback costs
                    // only its answer and its observers.
                    Err(unanswered) if self.interceptors.is_empty() => {
                        break 'served self.answer(unanswered, &request);
                    }
                    Err(unanswered) => {
                        let terminal = FallbackTerminal {
                            dispatch: Arc::clone(&self),
                            unanswered,
                        };
                        break 'served self
                            .chain(
                                &self.interceptors,
                                &terminal,
                                None,
                                self.catch_panics,
                                request,
                            )
                            .await;
                    }
                };

                let operation = &self.paths[at.path].operations[at.position];

                // A layer wraps the operation after routing, where an
                // interceptor runs.
                #[cfg(feature = "unchecked")]
                if !operation.unchecked_layers.is_empty() {
                    let layered = if operation.catch_panics {
                        Self::recovering_layers(Arc::clone(&self), at, request)
                    } else {
                        crate::unchecked::through_layers(
                            Arc::clone(&self),
                            at.path,
                            at.position,
                            request,
                        )
                    };
                    break 'served match layered.await {
                        Ok(response) => response,
                        Err(never) => match never {},
                    };
                }

                self.run(operation, self.route_at(at), request).await
            };

            self.finish(response, routed.ok(), started, head)
        }
    }

    /// Runs the operation at `at` through its unchecked layers under recovery.
    ///
    /// A layer runs where an interceptor does, so the operation's recovery
    /// covers it, its first `call` included: that is made inside the guarded
    /// future. The same box type as the unguarded path, so `serve`'s future
    /// awaits one shape whichever it took.
    #[cfg(feature = "unchecked")]
    fn recovering_layers(
        dispatch: Arc<Self>,
        at: Location,
        request: Request,
    ) -> crate::unchecked::BoxResponse {
        Box::pin(async move {
            let table = Arc::clone(&dispatch);
            let recovered = recover(async move {
                match crate::unchecked::through_layers(table, at.path, at.position, request).await {
                    Ok(response) => response,
                    Err(never) => match never {},
                }
            })
            .await;

            Ok(recovered.unwrap_or_else(|payload| {
                dispatch.report_panic(payload.as_ref(), Some(dispatch.route_at(at)));
                panic_response()
            }))
        })
    }

    /// Where the operation answering `request` sits, or why none does.
    ///
    /// Records what routing learned on the request and tells every observer
    /// about it, so all that is left of serving it is the waiting.
    fn route(&self, request: &mut Request) -> Result<Location, Unanswered> {
        let method = Method::from_wire_str(request.method().as_str());

        // Captures are ranges rather than borrows, so the request stays mutable.
        let (index, captures) = {
            let path = request.uri().path();
            let Ok(matched) = self.matcher.at(path) else {
                // RFC 9110 section 15.6.2: a 501 is about the server, not the
                // resource, so no 404 or redirect is owed first.
                let unanswered = if self.implements(method) {
                    Unanswered::Path
                } else {
                    Unanswered::Method { path: None }
                };
                return Err(self.unrouted(unanswered, request));
            };

            let index = *matched.value;
            let variables = self.paths[index].variables;
            let captures = (!variables.is_empty())
                .then(|| PathCaptures::new(path, variables, |name| matched.params.get(name)));

            (index, captures)
        };

        let entry = &self.paths[index];

        let Some(position) = method.and_then(|method| entry.position(method)) else {
            let path = self.implements(method).then_some(index);
            return Err(self.unrouted(Unanswered::Method { path }, request));
        };

        let operation = &entry.operations[position];
        let route = Route::new(&entry.template, &operation.operation_id, operation.method);

        let forwarded = self.forwarded(request);
        request.extensions_mut().insert(Routed {
            // The template, not the request path, to bound label cardinality.
            matched: Some(entry.matched.clone()),
            captures,
            forwarded,
            secured: operation.secured,
            links: self.links.clone(),
        });

        for observer in &self.observers {
            observer.on_request(request, Some(route), &self.context);
        }

        Ok(Location {
            path: index,
            position,
        })
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
    fn run<'a>(
        &'a self,
        operation: &'a Served<C>,
        route: Route<'a>,
        request: Request,
    ) -> impl Future<Output = Response> + Send + 'a {
        self.chain(
            &operation.interceptors,
            &*operation.terminal,
            Some(route),
            operation.catch_panics,
            request,
        )
    }

    /// Runs `interceptors` around `terminal`, with recovery if `catch_panics`
    /// asked for it; `route` is `None` around a fallback.
    ///
    /// The chain is started before the returned future exists, so that future
    /// holds the chain's and never the request it was started with.
    fn chain<'a>(
        &'a self,
        interceptors: &'a [Arc<dyn ErasedInterceptor<C>>],
        terminal: &'a dyn ErasedTerminal<C>,
        route: Option<Route<'a>>,
        catch_panics: bool,
        request: Request,
    ) -> impl Future<Output = Response> + Send + 'a {
        let next = Next::new(interceptors, terminal, &self.context, route);
        let started = if catch_panics {
            // Started inside the block, so that recovery also covers what
            // `Next::run` does before its future is first polled.
            Started::Recovering(recover(
                async move { next.run(request).await.into_response() },
            ))
        } else {
            Started::Unguarded(next.run(request))
        };

        async move {
            let mut response = match started {
                Started::Recovering(recovering) => match recovering.await {
                    Ok(response) => response,
                    Err(payload) => {
                        self.report_panic(payload.as_ref(), route);
                        panic_response()
                    }
                },
                Started::Unguarded(unguarded) => unguarded.await.into_response(),
            };

            // Taken whether or not anyone observes, so the marker never reaches
            // the driver.
            if let Some(payload) = take_recovered(&mut response) {
                self.report_panic(payload.as_ref(), route);
            }
            response
        }
    }

    /// Tells every observer about a recovered panic.
    fn report_panic(&self, payload: &(dyn Any + Send), route: Option<Route<'_>>) {
        for observer in &self.observers {
            observer.on_panic(payload, route);
        }
    }

    /// Records a request no operation matched, which is answered through the
    /// root router's own interceptors where it has any, and tells every
    /// observer either way.
    ///
    /// Without interceptors nothing is resolved or inserted, so a fallback
    /// costs only its answer and its observers.
    fn unrouted(&self, unanswered: Unanswered, request: &mut Request) -> Unanswered {
        // Resolved as for a routed request, so an interceptor reading the
        // origin reads the same answer on a 404 as on a 200.
        if !self.interceptors.is_empty() {
            let forwarded = self.forwarded(request);
            request.extensions_mut().insert(Routed {
                matched: None,
                captures: None,
                forwarded,
                secured: false,
                links: None,
            });
        }

        for observer in &self.observers {
            observer.on_request(request, None, &self.context);
        }

        unanswered
    }

    /// The fallback `unanswered` names, for `request`.
    fn answer(&self, unanswered: Unanswered, request: &Request) -> Response {
        match unanswered {
            Unanswered::Path => self.unmatched(request),
            Unanswered::Method { path } => method_refusal(
                path.map(|path| &self.paths[path].allow),
                &self.method_not_allowed,
            ),
        }
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
    ///
    /// `started` is `None` exactly when no observer is mounted, since only an
    /// observer reads it.
    fn finish(
        self: &Arc<Self>,
        response: Response,
        at: Option<Location>,
        started: Option<Instant>,
        head: bool,
    ) -> Response {
        let response = if head {
            without_content(response)
        } else {
            response
        };

        let Some(started) = started else {
            return response;
        };

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

/// Re-entered by index, since a `tower` future cannot borrow the table.
#[cfg(feature = "unchecked")]
impl<C: Send + Sync + 'static> crate::unchecked::Resume for Dispatch<C> {
    fn layers(&self, path: usize, position: usize) -> &[Arc<dyn crate::unchecked::ErasedLayer>] {
        &self.paths[path].operations[position].unchecked_layers
    }

    fn resume(
        self: Arc<Self>,
        path: usize,
        position: usize,
        request: Request,
    ) -> crate::unchecked::BoxResponse {
        Box::pin(async move {
            let entry = &self.paths[path];
            let operation = &entry.operations[position];
            let route = Route::new(&entry.template, &operation.operation_id, operation.method);
            Ok(self.run(operation, route, request).await)
        })
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
    let (mut parts, mut body) = response.into_parts();

    let bodiless = parts.status.is_informational()
        || parts.status == StatusCode::NO_CONTENT
        || parts.status == StatusCode::NOT_MODIFIED;
    if !bodiless && !parts.headers.contains_key(header::CONTENT_LENGTH) {
        if let Some(length) = body.exact_length().filter(|&length| length != 0) {
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
    let joined = crate::http::join_names(
        methods
            .iter()
            .flat_map(|method| {
                let head = (derives_head && *method == Method::Get).then_some(Method::Head);
                std::iter::once(*method).chain(head)
            })
            .map(Method::as_wire_str),
    );

    joined.unwrap_or_else(|| HeaderValue::from_static(""))
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

/// Leaks a path or a path variable name for the life of the process. Leaks once
/// per call: once for each path a build installs and once for each variable
/// occurrence in it, so each build leaks an amount bounded by its route table.
pub(crate) fn intern(name: &str) -> &'static str {
    Box::leak(name.to_owned().into_boxed_str())
}

#[cfg(test)]
mod tests;
