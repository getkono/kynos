//! Driving a router in-process, without a socket.
//!
//! The client also checks that the responses a test observed match what the
//! description promises, so a suite exercising every operation proves the
//! document truthful — see [`TestClient::assert_conformance`]. Where the
//! declared responses need several differently built services to produce, a
//! [`Coverage`] shared between their clients checks them together.

mod body;
mod conformance;

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use bytes::{Bytes, BytesMut};
use http_body_util::BodyExt;
use kynos_openapi::{
    Document, Method, model::body::mime_names::APPLICATION_PROBLEM_JSON as PROBLEM_JSON,
};
use serde_json::{Map, Value};

use crate::{
    extract::connection::Connection,
    http::{
        HeaderMap, HeaderName, HeaderValue, Method as HttpMethod, Request, Response, StatusCode,
        Uri, body::Body,
    },
    router::service::Service,
    test::{
        body::{Framed, RequestBody},
        conformance::{conformance, declared_keys, declared_response, matched_template},
    },
};

/// One response, as it was received, with the request target; the template is
/// matched against the description when an assertion runs.
#[derive(Debug)]
struct Observed {
    method: HttpMethod,
    /// The target as the test wrote it, for reporting.
    target: String,
    /// The target the service routed, which the template is matched by.
    uri: Uri,
    status: StatusCode,
    headers: HeaderMap,
    /// The whole body, or `None` while a [`TestStream`] has not read to its
    /// end, which it never will once dropped.
    body: Option<Bytes>,
}

/// Sends requests to a [`Service`] directly, through a configured
/// [`Server`](crate::server::Server) with
/// [`through_server`](Self::through_server), or to a spawned one over its
/// listener with [`connect`](Self::connect).
#[derive(Debug)]
pub struct TestClient<C> {
    transport: Transport<C>,
    /// Behind a lock because [`TestRequest::send`] borrows the client shared.
    observed: Mutex<Vec<Observed>>,
    /// Where every exchange is also recorded, when the client shares one.
    coverage: Option<Arc<Mutex<Vec<Exchange>>>>,
    /// The peer every request starts from.
    peer: Option<std::net::SocketAddr>,
    /// The fields every request starts from.
    headers: HeaderMap,
    /// What [`Transport::Direct`] holds a request body to between frames.
    #[cfg(feature = "server")]
    request_body_idle_timeout: IdleTimeout,
}

/// What a [`TestClient`] holds a request body to between frames.
#[cfg(feature = "server")]
#[derive(Clone, Copy, Debug)]
enum IdleTimeout {
    /// A server's default, and a moment for a
    /// [`stalled_body`](TestRequest::stalled_body).
    Default,
    /// What [`TestClient::with_request_body_idle_timeout`] set, for every
    /// body.
    Set(Option<std::time::Duration>),
}

impl<C> TestClient<C> {
    /// Wraps a built service.
    ///
    /// With the `server` feature, every request body is held to the idle
    /// timeout a server holds it to,
    /// [`Server::DEFAULT_REQUEST_BODY_IDLE_TIMEOUT`](crate::server::Server::DEFAULT_REQUEST_BODY_IDLE_TIMEOUT),
    /// unless [`with_request_body_idle_timeout`](Self::with_request_body_idle_timeout)
    /// says otherwise.
    #[must_use]
    pub fn new(service: Service<C>) -> Self {
        Self::over(Transport::Direct(Box::new(service)))
    }

    fn over(transport: Transport<C>) -> Self {
        Self {
            transport,
            observed: Mutex::new(Vec::new()),
            coverage: None,
            peer: None,
            headers: HeaderMap::new(),
            #[cfg(feature = "server")]
            request_body_idle_timeout: IdleTimeout::Default,
        }
    }

    /// Holds every request body to `timeout` between frames, as a server
    /// built with
    /// [`Server::request_body_idle_timeout(timeout)`](crate::server::Server::request_body_idle_timeout)
    /// does, so a stalled body is answered by whichever bound that server
    /// would answer it with. `None` holds it to nothing, leaving the
    /// operation's own bounds to answer.
    ///
    /// Until this is called, a body is held to
    /// [`Server::DEFAULT_REQUEST_BODY_IDLE_TIMEOUT`](crate::server::Server::DEFAULT_REQUEST_BODY_IDLE_TIMEOUT),
    /// and a [`stalled_body`](TestRequest::stalled_body) to a millisecond;
    /// once it is, `timeout` holds both. A second call replaces the first.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use kynos::{Router, test::TestClient};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = Router::<()>::new().build(())?;
    /// // As served by a server whose bodies have no idle timeout.
    /// let client = TestClient::new(service).with_request_body_idle_timeout(None);
    /// # let _ = client.with_request_body_idle_timeout(Some(Duration::from_secs(5)));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics when `timeout` is `Some(Duration::ZERO)`, which a server
    /// refuses too, and on a client built
    /// [`through_server`](Self::through_server) or
    /// [`connect`](Self::connect), whose server's own setting holds its
    /// bodies.
    #[cfg(feature = "server")]
    #[must_use]
    pub fn with_request_body_idle_timeout(mut self, timeout: Option<std::time::Duration>) -> Self {
        if let Some(built) = self.transport.served_by() {
            panic!(
                "a client built `{built}` holds a body to its server's \
                 `request_body_idle_timeout`; set it there"
            );
        }
        if let Err(error) = crate::server::request_body::validate_request_body_idle_timeout(timeout)
        {
            panic!("{error}");
        }
        self.request_body_idle_timeout = IdleTimeout::Set(timeout);
        self
    }

    /// The idle timeout [`Transport::Direct`] holds a request's body to.
    /// `stalled` says the body is
    /// [`stalled_body`](TestRequest::stalled_body)'s.
    #[cfg(feature = "server")]
    fn request_body_idle_limit(&self, stalled: bool) -> Option<std::time::Duration> {
        match self.request_body_idle_timeout {
            IdleTimeout::Set(timeout) => timeout,
            IdleTimeout::Default if stalled => Some(STALLED_BODY_IDLE_TIMEOUT),
            IdleTimeout::Default => Some(crate::server::Server::DEFAULT_REQUEST_BODY_IDLE_TIMEOUT),
        }
    }

    /// The service that answers, whose description assertions read.
    fn service(&self) -> &Service<C> {
        match &self.transport {
            Transport::Direct(service) => service,
            #[cfg(feature = "server")]
            Transport::Served { server, .. } => server.served(),
            #[cfg(feature = "server")]
            Transport::Connected { service, .. } => service,
        }
    }

    /// Panics, naming `setter`, on a client built
    /// [`connect`](Self::connect), whose requests leave from their own
    /// socket's address.
    #[cfg(feature = "server")]
    fn refuse_peer_over_a_socket(&self, setter: &str) {
        if let Transport::Connected { .. } = self.transport {
            panic!(
                "a client built `connect` sends from its own socket's address, \
                 which `{setter}` cannot set; use `through_server` to test a peer"
            );
        }
    }

    /// Records every response this client receives into `coverage` as well.
    ///
    /// The client's own assertions still read only what it received; the
    /// shared record is read by [`Coverage::assert_declared_responses_covered`]
    /// or [`Coverage::assert_covered`].
    /// A second call replaces the first.
    #[must_use]
    pub fn with_coverage(mut self, coverage: &Coverage) -> Self {
        self.coverage = Some(Arc::clone(&coverage.exchanges));
        self
    }

    /// Sends every request from `peer`, unless the request sets its own with
    /// [`TestRequest::peer`].
    ///
    /// For a service every request of a suite reaches through the same
    /// client address: a rate limiter keyed by client, or a trusted-proxy
    /// policy. A second call replaces the first.
    ///
    /// # Panics
    ///
    /// Panics on a client built [`connect`](Self::connect), whose requests
    /// leave from their own socket's address.
    #[must_use]
    pub fn with_peer(mut self, peer: std::net::SocketAddr) -> Self {
        #[cfg(feature = "server")]
        self.refuse_peer_over_a_socket("with_peer");
        self.peer = Some(peer);
        self
    }

    /// Sends the field `name: value` on every request, unless the request
    /// sets a field of the same name itself.
    ///
    /// A request's own field replaces the default whole, whichever setter
    /// wrote it: [`TestRequest::header`], [`bearer`](TestRequest::bearer) for
    /// `Authorization`, a body's `Content-Type`, or the `Cookie` field
    /// [`cookie`](TestRequest::cookie) assembles. A second call naming the
    /// same field replaces the first.
    ///
    /// # Panics
    ///
    /// Panics when `name` or `value` is not one HTTP can carry.
    #[must_use]
    pub fn with_default_header(mut self, name: &str, value: &str) -> Self {
        let (name, value) = field(name, value);
        self.headers.insert(name, value);
        self
    }

    /// Begins a `GET` request.
    #[must_use]
    pub fn get(&self, path: &str) -> TestRequest<'_, C> {
        self.request(HttpMethod::GET, path)
    }

    /// Begins a `POST` request.
    #[must_use]
    pub fn post(&self, path: &str) -> TestRequest<'_, C> {
        self.request(HttpMethod::POST, path)
    }

    /// Begins a `PUT` request.
    #[must_use]
    pub fn put(&self, path: &str) -> TestRequest<'_, C> {
        self.request(HttpMethod::PUT, path)
    }

    /// Begins a `PATCH` request.
    #[must_use]
    pub fn patch(&self, path: &str) -> TestRequest<'_, C> {
        self.request(HttpMethod::PATCH, path)
    }

    /// Begins a `DELETE` request.
    #[must_use]
    pub fn delete(&self, path: &str) -> TestRequest<'_, C> {
        self.request(HttpMethod::DELETE, path)
    }

    /// Begins a `HEAD` request.
    #[must_use]
    pub fn head(&self, path: &str) -> TestRequest<'_, C> {
        self.request(HttpMethod::HEAD, path)
    }

    /// Begins an `OPTIONS` request.
    #[must_use]
    pub fn options(&self, path: &str) -> TestRequest<'_, C> {
        self.request(HttpMethod::OPTIONS, path)
    }

    /// Begins a `TRACE` request.
    #[must_use]
    pub fn trace(&self, path: &str) -> TestRequest<'_, C> {
        self.request(HttpMethod::TRACE, path)
    }

    /// Begins a `QUERY` request.
    ///
    /// The method `#[kynos::query]` registers. Behind `openapi32`, the only
    /// build that can route `QUERY`; [`method`](Self::method) still sends it
    /// in any build.
    #[cfg(feature = "openapi32")]
    #[must_use]
    pub fn query(&self, path: &str) -> TestRequest<'_, C> {
        self.request(
            HttpMethod::from_bytes(b"QUERY").expect("`QUERY` is a method token"),
            path,
        )
    }

    /// Begins a request with any method.
    ///
    /// For a method Kynos routes that this type has no shortcut for.
    #[must_use]
    pub fn method(&self, method: HttpMethod, path: &str) -> TestRequest<'_, C> {
        self.request(method, path)
    }

    fn request(&self, method: HttpMethod, path: &str) -> TestRequest<'_, C> {
        TestRequest {
            client: self,
            method,
            path: path.to_owned(),
            headers: self.headers.clone(),
            body: RequestBody::Whole(Bytes::new()),
            content_length: None,
            peer: self.peer,
            cookies: Vec::new(),
        }
    }

    /// Records one exchange as its head arrived, into the shared coverage
    /// too, and returns where it was observed, for a [`TestStream`] to file
    /// the body under once it ends.
    fn record(
        &self,
        method: &HttpMethod,
        target: &str,
        uri: Uri,
        parts: &::http::response::Parts,
        body: Option<Bytes>,
    ) -> usize {
        if let Some(coverage) = &self.coverage {
            push(
                coverage,
                Exchange {
                    method: method.clone(),
                    uri: uri.clone(),
                    status: parts.status,
                },
            );
        }

        let mut recorded = self.recorded();
        recorded.push(Observed {
            method: method.clone(),
            target: target.to_owned(),
            uri,
            status: parts.status,
            headers: parts.headers.clone(),
            body,
        });
        recorded.len() - 1
    }

    /// Reads back what has been observed so far.
    fn recorded(&self) -> std::sync::MutexGuard<'_, Vec<Observed>> {
        self.observed
            .lock()
            .expect("a test client whose recorder panicked cannot be asserted on")
    }

    /// Asserts that every response this client has seen conforms to the
    /// description.
    ///
    /// Each observed response is checked against the `Responses` entry for its
    /// operation and status: that the status is declared at all, that the body
    /// validates against the declared schema, and that every declared required
    /// header was sent.
    ///
    /// A body or a `Content-Type` arriving under a response that declares no
    /// representation is reported too.
    ///
    /// A `HEAD` is checked against the operation that answered it: its own
    /// `head`, or the `get` of a path declaring none. It carries no content,
    /// so only its `Content-Type` is held to the declared representation.
    ///
    /// # Panics
    ///
    /// Panics listing every response that did not conform.
    pub fn assert_conformance(&self) {
        let document = self.service().openapi();
        let mut failures = Vec::new();

        for record in self.recorded().iter() {
            for reason in conformance(document, record) {
                failures.push(format!(
                    "  {} {} -> {}: {reason}",
                    record.method,
                    record.target,
                    record.status.as_u16()
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "{} did not conform to the description:\n{}",
            responses(failures.len()),
            failures.join("\n")
        );
    }

    /// Asserts that every declared response was exercised at least once.
    ///
    /// Coverage over the *contract* rather than over the code: it finds the 409
    /// that the description promises and no test has ever produced. A `HEAD`
    /// answered by a path's `get` does not count toward that operation.
    ///
    /// # Panics
    ///
    /// Panics listing every declared response that was never seen.
    pub fn assert_declared_responses_covered(&self) {
        let recorded = self.recorded();
        assert_covered(
            self.service().openapi(),
            recorded
                .iter()
                .map(|record| (&record.method, &record.uri, record.status)),
        );
    }
}

#[cfg(feature = "server")]
impl<C: 'static> TestClient<C> {
    /// Sends every request over a fresh
    /// [`Server::serve_io`](crate::server::Server::serve_io) connection on an
    /// in-memory duplex, so the server's transport settings apply: its
    /// protocol configs, header-read timeout and request-body idle timeout.
    /// Observation, conformance and coverage work as for [`new`](Self::new).
    ///
    /// The request is written as HTTP/1.1 where the `http1` feature is
    /// compiled, and as prior-knowledge HTTP/2 otherwise. What it inherits
    /// from `serve_io`:
    ///
    /// - Listener-only settings do not apply: the connection limit, TCP
    ///   keepalive, and graceful shutdown and its timeout.
    ///   [`connect`](Self::connect) sends to a listener, where they do.
    /// - Each connection carries one request and closes once its response
    ///   body has been read; a dropped [`send`](TestRequest::send) or
    ///   [`TestStream`] closes it without draining.
    /// - The connection is recorded between [`TestRequest::peer`], the
    ///   client's [`with_peer`](Self::with_peer), or `127.0.0.1:0` where
    ///   neither is set, and a local `127.0.0.1:0`.
    /// - A body ending short of its
    ///   [`content_length`](TestRequest::content_length) aborts the message,
    ///   as a client whose connection broke does.
    ///
    /// ```
    /// use kynos::{Router, server::Server, test::TestClient};
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let service = Router::<()>::new().build(())?;
    /// let client = TestClient::through_server(Server::new(service));
    /// // No operation answers GET anywhere on an empty router.
    /// client.get("/").send().await.assert_status(501);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics when `serve_io` refuses `server`: a value
    /// [`prepare`](crate::server::Server::prepare) refuses, or TLS.
    #[must_use]
    pub fn through_server(server: crate::server::Server<C>) -> Self {
        if let Err(error) = server.check_servable() {
            panic!("a server `serve_io` refuses cannot be tested through: {error}");
        }
        Self::over(Transport::Served {
            server: Box::new(server),
            exchange: crate::server::Server::exchange,
        })
    }

    /// Sends every request over a fresh TCP connection to a spawned server's
    /// first [`local_addr`](crate::server::Running::local_addr), so every
    /// setting applies, the listener's included: the connection limit, TCP
    /// keepalive, and graceful shutdown. Conformance and coverage read the
    /// description of the service `running`
    /// [serves](crate::server::Running::service).
    ///
    /// An unspecified address, `0.0.0.0` or `::`, is reached through its
    /// family's loopback. The request is written as
    /// [`through_server`](Self::through_server) writes it, and its
    /// connection closes as that one does: once its response body has been
    /// read, or a dropped [`send`](TestRequest::send) or [`TestStream`] has
    /// hung up. The client borrows nothing from `running`, so
    /// [`Running::shutdown`](crate::server::Running::shutdown) may begin
    /// while it is held; a request sent once the listener has closed panics
    /// with the refusal.
    ///
    /// What a socket fixes for itself, the client cannot set: a request
    /// leaves from the connection's own address, loopback for a loopback
    /// listener, so
    /// [`with_peer`](Self::with_peer) and [`TestRequest::peer`] panic, and
    /// [`TestRequest::send_timed`] panics, since the real socket's
    /// readiness is not on the paused clock it measures. Nor does it write a
    /// PROXY protocol header, so a server trusting loopback as its proxy
    /// refuses every connection.
    ///
    /// ```
    /// use kynos::{Router, server::Server, test::TestClient};
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let running = Server::new(Router::<()>::new().build(())?)
    ///     .bind(("127.0.0.1", 0))
    ///     .prepare()
    ///     .await?
    ///     .spawn();
    /// let client = TestClient::connect(&running);
    /// client.get("/").send().await.assert_status(501);
    /// running.shutdown().await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics when `running` has no TCP listener, as one serving only Unix
    /// domain sockets has, and when its listeners speak TLS, since the
    /// client writes plaintext.
    #[must_use]
    pub fn connect(running: &crate::server::Running<C>) -> Self {
        let address = running.local_addr().unwrap_or_else(|| {
            panic!("a server with no TCP listener cannot be connected to; `connect` sends over TCP")
        });
        assert!(
            !running.serves_tls(),
            "a server whose listeners speak TLS cannot be connected to; `connect` writes plaintext"
        );
        Self::over(Transport::Connected {
            address: reachable(address),
            service: running.shared_service(),
        })
    }
}

/// Where a client reaches a listener bound to `address`: its family's
/// loopback where `address` is unspecified, which names no host to connect
/// to, and `address` itself otherwise.
#[cfg(feature = "server")]
fn reachable(mut address: std::net::SocketAddr) -> std::net::SocketAddr {
    match address.ip() {
        std::net::IpAddr::V4(ip) if ip.is_unspecified() => {
            address.set_ip(std::net::Ipv4Addr::LOCALHOST.into());
        }
        std::net::IpAddr::V6(ip) if ip.is_unspecified() => {
            address.set_ip(std::net::Ipv6Addr::LOCALHOST.into());
        }
        _ => {}
    }
    address
}

/// Where a [`TestClient`] sends its requests. Each variant boxes what it
/// holds, since a service and a server differ widely in size.
#[derive(Debug)]
enum Transport<C> {
    /// To [`Service::call`].
    Direct(Box<Service<C>>),
    /// Over one fresh `serve_io` connection each.
    #[cfg(feature = "server")]
    Served {
        server: Box<crate::server::Server<C>>,
        /// `Server::exchange`, taken where `C: 'static` is known so sending
        /// needs no such bound.
        exchange: fn(&crate::server::Server<C>, Request, Connection) -> Exchanged,
    },
    /// Over one fresh TCP connection each, to a spawned server's listener.
    #[cfg(feature = "server")]
    Connected {
        address: std::net::SocketAddr,
        /// The service that listener serves, whose description assertions
        /// read.
        service: Arc<Service<C>>,
    },
}

impl<C> Transport<C> {
    /// The constructor of a client whose server holds its bodies to its own
    /// timer, or `None` for [`Self::Direct`].
    #[cfg(feature = "server")]
    fn served_by(&self) -> Option<&'static str> {
        match self {
            Self::Direct(_) => None,
            Self::Served { .. } => Some("through_server"),
            Self::Connected { .. } => Some("connect"),
        }
    }

    /// Sends `request` from `peer`, failing only where a connection ended
    /// without a response. [`Self::Direct`] holds the body to `idle` between
    /// frames, as a server would; a served one is held to its server's own.
    async fn answer(
        &self,
        mut request: Request,
        peer: Option<std::net::SocketAddr>,
        idle: Option<std::time::Duration>,
    ) -> std::io::Result<Response> {
        match self {
            Self::Direct(service) => {
                if let Some(peer) = peer {
                    // The same extension the server inserts per connection.
                    request
                        .extensions_mut()
                        .insert(Connection::from_peer(peer, IN_PROCESS));
                }
                // Held as a server holds it, so a stalled body is answered as
                // a served one is.
                #[cfg(feature = "server")]
                let (request, stall) = {
                    let (parts, body) = request.into_parts();
                    let (body, stall) = crate::server::request_body::bounded(
                        body,
                        idle,
                        crate::http::Version::HTTP_11,
                        std::convert::identity,
                    );
                    (Request::from_parts(parts, body), stall)
                };
                // Only a server holds a body to a timer.
                #[cfg(not(feature = "server"))]
                let _ = idle;
                let method = request.method().clone();
                let response = service.call(request).await;
                #[cfg(feature = "server")]
                let response = crate::server::request_body::answer(response, stall);
                Ok(with_content_length(&method, response))
            }
            #[cfg(feature = "server")]
            Self::Served { server, exchange } => {
                let connection = Connection::from_peer(peer.unwrap_or(IN_PROCESS), IN_PROCESS);
                exchange(server, request, connection).await
            }
            // The peer is the socket's own; `with_peer` and `peer` refuse
            // to set one.
            #[cfg(feature = "server")]
            Self::Connected { address, .. } => {
                crate::server::connection::test_client::exchange_at(*address, request).await
            }
        }
    }
}

/// `response` with the `Content-Length` a connection would frame it with,
/// so an in-process response carries the field a served one does.
///
/// Set only where the body states its exact length, as a connection learns
/// it, and never over a `Content-Length` or `Transfer-Encoding` already
/// there. RFC 9110 section 8.6 leaves a response to `HEAD` and a 304 the
/// length of the response they stand in for, which the body here is not, and
/// forbids one on a 1xx, a 204 and a 2xx to `CONNECT`.
fn with_content_length(method: &HttpMethod, mut response: Response) -> Response {
    let status = response.status();
    if *method == HttpMethod::HEAD
        || status.is_informational()
        || status == StatusCode::NO_CONTENT
        || status == StatusCode::NOT_MODIFIED
        || (*method == HttpMethod::CONNECT && status.is_success())
        || response
            .headers()
            .contains_key(crate::http::header::CONTENT_LENGTH)
        || response
            .headers()
            .contains_key(crate::http::header::TRANSFER_ENCODING)
    {
        return response;
    }
    if let Some(length) = response.body_mut().exact_length() {
        response.headers_mut().insert(
            crate::http::header::CONTENT_LENGTH,
            HeaderValue::from(length),
        );
    }
    response
}

/// How long [`Transport::Direct`] waits on a
/// [`stalled_body`](TestRequest::stalled_body) before failing its read: the
/// shortest wait a timer measures, since no frame can end it.
#[cfg(feature = "server")]
const STALLED_BODY_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1);

/// A response [`Transport::Served`] is answered with.
#[cfg(feature = "server")]
type Exchanged =
    std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<Response>> + Send>>;

/// Where neither the test nor its client says who a served request is from,
/// and where every request arrives.
const IN_PROCESS: std::net::SocketAddr = std::net::SocketAddr::V4(std::net::SocketAddrV4::new(
    std::net::Ipv4Addr::LOCALHOST,
    0,
));

/// The responses any number of [`TestClient`]s received, checked together
/// against one description.
///
/// A description may declare responses no single service can produce: one
/// built with authentication and one without, or with an optional subsystem
/// present and absent. Each of those clients is built
/// [`with_coverage`](TestClient::with_coverage) over the same `Coverage`, and
/// the declared responses are asserted once, after all of them ran, against
/// the description that declares them all.
///
/// A clone is the same record, not a copy of it: what one clone's clients
/// record, every clone reads, and every clone is bound to the same document.
#[derive(Clone, Debug, Default)]
pub struct Coverage {
    exchanges: Arc<Mutex<Vec<Exchange>>>,
    document: Option<Arc<Document>>,
}

impl Coverage {
    /// A record no client has written to yet, bound to no document: it is
    /// asserted with
    /// [`assert_declared_responses_covered`](Self::assert_declared_responses_covered).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A record no client has written to yet, bound to `document`: it is
    /// asserted with [`assert_covered`](Self::assert_covered).
    ///
    /// `document` is usually the one the application publishes, built with
    /// every optional part present. An [`Arc<Document>`] is shared rather
    /// than copied.
    #[must_use]
    pub fn for_document(document: impl Into<Arc<Document>>) -> Self {
        Self {
            exchanges: Arc::default(),
            document: Some(document.into()),
        }
    }

    /// Records one exchange made outside a [`TestClient`], for
    /// [`assert_declared_responses_covered`](Self::assert_declared_responses_covered)
    /// or [`assert_covered`](Self::assert_covered) to count.
    ///
    /// `path` is the concrete request target, matched against the
    /// description when asserted, as a client's own exchanges are.
    ///
    /// # Panics
    ///
    /// Panics when `path` is not a request target.
    pub fn record(&self, method: &HttpMethod, path: &str, status: StatusCode) {
        let uri = path
            .parse::<Uri>()
            .unwrap_or_else(|_| panic!("`{path}` is not a request target"));
        push(
            &self.exchanges,
            Exchange {
                method: method.clone(),
                uri,
                status,
            },
        );
    }

    /// Asserts that every response `document` declares was exercised by at
    /// least one client recording here.
    ///
    /// A response is matched against `document` by its method, path and
    /// status, whichever client received it, so `document` may be any one
    /// service's description or one built for the purpose. As with
    /// [`TestClient::assert_declared_responses_covered`], a `HEAD` answered by
    /// a path's `get` does not count toward that operation.
    ///
    /// # Panics
    ///
    /// Panics listing every declared response that was never seen.
    pub fn assert_declared_responses_covered(&self, document: &Document) {
        self.assert_against(document);
    }

    /// Asserts that every response the document given to
    /// [`for_document`](Self::for_document) declares was exercised by at
    /// least one client recording here, as
    /// [`assert_declared_responses_covered`](Self::assert_declared_responses_covered)
    /// does for an explicit document.
    ///
    /// # Panics
    ///
    /// Panics when this record was not built
    /// [`for_document`](Self::for_document), and otherwise listing every
    /// declared response that was never seen.
    pub fn assert_covered(&self) {
        let document = self.document.as_deref().expect(
            "a Coverage built without a document has nothing to assert against; \
             build it with `Coverage::for_document`, or name the document with \
             `assert_declared_responses_covered`",
        );
        self.assert_against(document);
    }

    /// Asserts the recorded exchanges cover what `document` declares.
    fn assert_against(&self, document: &Document) {
        let exchanges = self
            .exchanges
            .lock()
            .expect("a coverage record whose recorder panicked cannot be asserted on");
        assert_covered(
            document,
            exchanges
                .iter()
                .map(|exchange| (&exchange.method, &exchange.uri, exchange.status)),
        );
    }
}

/// What a [`Coverage`] keeps of one exchange: enough to match it against a
/// description later.
#[derive(Debug)]
struct Exchange {
    method: HttpMethod,
    uri: Uri,
    status: StatusCode,
}

/// Adds `exchange` to a shared coverage record.
fn push(exchanges: &Mutex<Vec<Exchange>>, exchange: Exchange) {
    exchanges
        .lock()
        .expect("a coverage record whose recorder panicked cannot be written to")
        .push(exchange);
}

/// Asserts that every response `document` declares is among `received`.
fn assert_covered<'r>(
    document: &Document,
    received: impl Iterator<Item = (&'r HttpMethod, &'r Uri, StatusCode)>,
) {
    let exercised: BTreeSet<(&str, Method, String)> = received
        .filter_map(|(method, uri, status)| {
            let template = matched_template(document, uri)?;
            let method = Method::from_wire_str(method.as_str())?;
            let operation = document.paths.items.get(template)?.operation(method)?;
            let (key, _) = declared_response(&operation.responses, status.as_u16())?;
            Some((template, method, key))
        })
        .collect();

    let mut missing = Vec::new();
    for (template, item) in &document.paths.items {
        for (method, operation) in item.operations() {
            for key in declared_keys(&operation.responses) {
                if !exercised.contains(&(template.as_str(), method, key.clone())) {
                    missing.push(format!("  {} {template} -> {key}", method.as_wire_str()));
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "{} declared but never exercised:\n{}",
        responses(missing.len()),
        missing.join("\n")
    );
}

/// A request under construction.
#[derive(Debug)]
pub struct TestRequest<'a, C> {
    client: &'a TestClient<C>,
    method: HttpMethod,
    path: String,
    headers: HeaderMap,
    body: RequestBody,
    /// The length the body is framed by, when the test declared one.
    content_length: Option<u64>,
    /// Who the request came from, for a service that reads one.
    peer: Option<std::net::SocketAddr>,
    /// Cookies, accumulated so several become one `Cookie` field.
    cookies: Vec<(String, String)>,
}

impl<'a, C> TestRequest<'a, C> {
    /// Sets a header.
    ///
    /// # Panics
    ///
    /// Panics when `name` or `value` is not one HTTP can carry.
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        let (name, value) = field(name, value);
        self.headers.insert(name, value);
        self
    }

    /// Presents `token` as `Authorization: Bearer <token>` (RFC 6750
    /// section 2.1).
    ///
    /// The token is sent as written, not held to the section's `b64token`
    /// grammar, so a test can also send one a service must refuse.
    ///
    /// # Panics
    ///
    /// Panics when `token` leaves the field no header value.
    #[must_use]
    pub fn bearer(self, token: &str) -> Self {
        self.header("authorization", &format!("Bearer {token}"))
    }

    /// Sets a header to a value given as octets.
    ///
    /// For a value [`header`](Self::header) cannot spell: one holding
    /// `obs-text` (RFC 9110 section 5.5), octets `0x80` to `0xFF` that need not
    /// be UTF-8, as a test of how a service treats an opaque `Host` sends.
    ///
    /// # Panics
    ///
    /// Panics when `name` is not a header name, or when `value` holds an octet
    /// no field value can: a control character other than horizontal tab.
    #[must_use]
    pub fn header_bytes(mut self, name: &str, value: &[u8]) -> Self {
        let name = HeaderName::from_bytes(name.as_bytes())
            .unwrap_or_else(|_| panic!("`{name}` is not a header name"));
        let value = HeaderValue::from_bytes(value)
            .unwrap_or_else(|_| panic!("`{}` is not a header value", value.escape_ascii()));
        self.headers.insert(name, value);
        self
    }

    /// Sets a JSON body.
    ///
    /// # Panics
    ///
    /// Panics when `body` cannot be serialized to JSON: a `Serialize`
    /// implementation that fails itself, or a map with a key JSON cannot spell
    /// as a string, such as a tuple, struct, `None` or unit key, or a NaN or
    /// infinite float (so a `HashMap<(u8, u8), _>` panics here).
    #[cfg(feature = "json")]
    #[must_use]
    pub fn json<T: serde::Serialize>(mut self, body: &T) -> Self {
        self.body = RequestBody::Whole(Bytes::from(
            serde_json::to_vec(body).expect("a serializable request body"),
        ));
        self.headers.insert(
            crate::http::header::CONTENT_TYPE,
            HeaderValue::from_static(kynos_openapi::model::body::mime_names::APPLICATION_JSON),
        );
        self
    }

    /// Adds a query string to the target, encoded from a serializable value.
    ///
    /// Not `query`, which [`TestClient::query`] uses for the `QUERY` method.
    /// Appends to whatever query the path already carries.
    ///
    /// # Panics
    ///
    /// Panics when `value` cannot be a query string, which a test writing a
    /// struct of scalars cannot cause.
    #[cfg(feature = "form")]
    #[must_use]
    pub fn query_string<T: serde::Serialize>(mut self, value: &T) -> Self {
        let encoded = serde_html_form::to_string(value).expect("a serializable query");
        if !encoded.is_empty() {
            let separator = if self.path.contains('?') { '&' } else { '?' };
            self.path.push(separator);
            self.path.push_str(&encoded);
        }
        self
    }

    /// Sends a cookie.
    ///
    /// Accumulated rather than set: RFC 6265 section 5.4 puts every cookie in
    /// one `Cookie` field separated by `; `.
    /// A name or value that leaves that field no header value panics in
    /// [`send`](Self::send), not here.
    #[must_use]
    pub fn cookie(mut self, name: &str, value: &str) -> Self {
        self.cookies.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Says who the request came from.
    ///
    /// Without one, a service reading a peer address sees the client's
    /// [`with_peer`](TestClient::with_peer), or the in-process default where
    /// the client sets none; set it to test a rate limiter keyed by client, or
    /// a trusted-proxy policy.
    ///
    /// # Panics
    ///
    /// Panics on a client built [`connect`](TestClient::connect), whose
    /// requests leave from their own socket's address.
    #[must_use]
    pub fn peer(mut self, address: std::net::SocketAddr) -> Self {
        #[cfg(feature = "server")]
        self.client.refuse_peer_over_a_socket("peer");
        self.peer = Some(address);
        self
    }

    /// Sets a raw body, and the media type it is in.
    ///
    /// # Panics
    ///
    /// Panics when `media_type` is not a header value.
    #[must_use]
    pub fn body(mut self, media_type: &str, bytes: impl Into<Bytes>) -> Self {
        self.body = RequestBody::Whole(bytes.into());
        self.content_type(media_type)
    }

    /// Sets a body that arrives as `chunks` yields them, and the media type it
    /// is in.
    ///
    /// Each chunk reaches the service as one frame when the stream yields it,
    /// so a stream that waits between chunks is a slow client and one that
    /// never yields again is a stalled one. With the `server` feature, a read
    /// that waits as long as the client's
    /// [`request_body_idle_timeout`](TestClient::with_request_body_idle_timeout),
    /// a server's default unless set, fails as a served body's does, so the
    /// stall is answered with the operation's 408;
    /// [`stalled_body`](Self::stalled_body) reaches it at once. Without
    /// [`content_length`](Self::content_length) the body declares no length,
    /// as a chunked one does, and ends when the stream does.
    ///
    /// # Panics
    ///
    /// Panics when `media_type` is not a header value.
    #[must_use]
    pub fn body_stream<S>(mut self, media_type: &str, chunks: S) -> Self
    where
        S: futures_core::Stream<Item = Bytes> + Send + 'static,
    {
        self.body = RequestBody::Streamed(Box::pin(chunks));
        self.content_type(media_type)
    }

    /// Sets a body that never arrives, declared one octet long, and the media
    /// type it would be in: a client that sent its head and stalled.
    ///
    /// The read fails with [`BodyStalled`](crate::http::body::BodyStalled)
    /// as a served one does, so an operation reading its body answers with its
    /// declared 408, carrying `Connection: close`. A client built with
    /// [`TestClient::new`] fails the read once the service has waited a
    /// millisecond, on the clock however it runs, so the 408 needs no paused
    /// clock and no wall-clock wait; one given
    /// [`with_request_body_idle_timeout`](TestClient::with_request_body_idle_timeout)
    /// waits that timeout instead, as a served body would. One built
    /// [`through_server`](TestClient::through_server) or
    /// [`connect`](TestClient::connect) sends it on the wire, where the
    /// server's own
    /// [`request_body_idle_timeout`](crate::server::Server::request_body_idle_timeout)
    /// ends the wait: give that server a short one, and with `None` the
    /// request is never answered.
    ///
    /// # Panics
    ///
    /// Panics when `media_type` is not a header value.
    #[cfg(feature = "server")]
    #[must_use]
    pub fn stalled_body(mut self, media_type: &str) -> Self {
        self.body = RequestBody::Stalled;
        self.content_length(1).content_type(media_type)
    }

    /// Sets a body that sends `first` and then nothing, never ending, and
    /// the media type it is in: a client that sent part of its body and went
    /// quiet.
    ///
    /// Without [`content_length`](Self::content_length) the body declares no
    /// length, as a chunked one whose last chunk never comes. With one, it
    /// owes the octets `first` leaves; an empty `first` declares a body none
    /// of which arrives, which an operation refuses from the head where the
    /// length is past its limit.
    ///
    /// The stall is held to the client's
    /// [`request_body_idle_timeout`](TestClient::with_request_body_idle_timeout),
    /// a server's default unless set, so the bound that answers it is the
    /// one a served body meets: the server's 408, or the operation's own
    /// deadline where that comes first. Unlike
    /// [`stalled_body`](Self::stalled_body), it is never cut to a moment,
    /// so [`send_timed`](Self::send_timed) reads how long the answer took.
    ///
    /// ```
    /// # async fn run(client: kynos::test::TestClient<()>) {
    /// let (reply, waited) = client
    ///     .post("/items")
    ///     .stalled_after("application/json", r#"{"name": "#)
    ///     .content_length(64)
    ///     .send_timed()
    ///     .await;
    /// reply.assert_status(408);
    /// assert!(waited >= kynos::server::Server::DEFAULT_REQUEST_BODY_IDLE_TIMEOUT);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics when `media_type` is not a header value, and in
    /// [`send`](Self::send) when `first` already fills the declared length,
    /// since such a body arrives whole rather than stalling.
    #[cfg(feature = "server")]
    #[must_use]
    pub fn stalled_after(mut self, media_type: &str, first: impl Into<Bytes>) -> Self {
        self.body = RequestBody::StalledAfter(first.into());
        self.content_type(media_type)
    }

    /// Declares the body's length: sends it as `Content-Length`, and frames
    /// the body by it.
    ///
    /// The service reads the body as a server reads a framed message: it ends
    /// after `length` octets, and whatever a test sends past them is never
    /// read. A body that ends short of `length` fails part-way, as a
    /// connection closed mid-message does. So a test can declare 64 octets,
    /// send 10, and leave the service waiting for the rest.
    #[must_use]
    pub fn content_length(mut self, length: u64) -> Self {
        self.content_length = Some(length);
        self.headers.insert(
            crate::http::header::CONTENT_LENGTH,
            HeaderValue::from(length),
        );
        self
    }

    fn content_type(mut self, media_type: &str) -> Self {
        self.headers.insert(
            crate::http::header::CONTENT_TYPE,
            HeaderValue::from_str(media_type)
                .unwrap_or_else(|_| panic!("`{media_type}` is not a media type")),
        );
        self
    }

    /// Sets a `text/plain` body.
    #[must_use]
    pub fn text(self, body: &str) -> Self {
        self.body("text/plain; charset=utf-8", Bytes::from(body.to_owned()))
    }

    /// Sets a form-encoded body.
    ///
    /// # Panics
    ///
    /// Panics when `body` cannot be form-encoded.
    #[cfg(feature = "form")]
    #[must_use]
    pub fn form<T: serde::Serialize>(self, body: &T) -> Self {
        let encoded = serde_html_form::to_string(body).expect("a serializable form body");
        self.body(
            kynos_openapi::model::body::mime_names::APPLICATION_FORM_URLENCODED,
            Bytes::from(encoded.into_bytes()),
        )
    }

    /// Sends the request.
    ///
    /// The response arrives with the `Content-Length` a connection would
    /// frame it with, by any client: one built
    /// [`through_server`](TestClient::through_server) or
    /// [`connect`](TestClient::connect) reads it off the wire,
    /// and one built with [`TestClient::new`] states the body's length where
    /// the body knows it and RFC 9110 section 8.6 allows one, keeping any the
    /// service set. A streamed body of unknown length states none, as a
    /// chunked message carries none.
    ///
    /// # Panics
    ///
    /// Panics when the path is not a request target, when a
    /// [`cookie`](Self::cookie) leaves the `Cookie` field no header value, or
    /// when the response body fails part-way through, or, for a client built
    /// [`through_server`](TestClient::through_server) or
    /// [`connect`](TestClient::connect), when the connection is refused or
    /// ends without a response.
    pub async fn send(self) -> TestResponse {
        let Answered {
            client,
            method,
            path,
            uri,
            response,
        } = self.answered().await;

        let (parts, body) = response.into_parts();
        let bytes = body
            .collect()
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "{method} {path} -> {}: the response body failed part-way: {error}",
                    parts.status.as_u16()
                )
            })
            .to_bytes();

        client.record(&method, &path, uri, &parts, Some(bytes.clone()));

        TestResponse {
            method,
            path,
            response: Response::from_parts(parts, Body::from_bytes(bytes.clone())),
            body: bytes,
        }
    }

    /// Sends the request, and returns as soon as the response head arrives,
    /// with the body still arriving.
    ///
    /// For a body that does not end on its own, an event stream or a long
    /// poll: a test reads an event, acts, and reads the next, then
    /// [`finish`](TestStream::finish)es the body or drops the stream, which
    /// hangs up as a departing client does.
    ///
    /// The exchange is recorded once its head arrives, so a dropped stream
    /// counts toward [`TestClient::assert_declared_responses_covered`] and a
    /// shared [`Coverage`]. [`TestClient::assert_conformance`] checks its
    /// status and fields, and its body once the stream has read the body to
    /// its end, by [`finish`](TestStream::finish) or by reading until `None`.
    ///
    /// ```
    /// use kynos::{Router, test::TestClient};
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = TestClient::new(Router::<()>::new().build(())?);
    /// let mut stream = client.get("/").send_streaming().await;
    /// assert_eq!(stream.status(), 501);
    /// while let Some(chunk) = stream.next_chunk().await {
    ///     assert!(!chunk.is_empty());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics as [`send`](Self::send) does before the head arrives.
    pub async fn send_streaming(self) -> TestStream<'a> {
        let Answered {
            client,
            method,
            path,
            uri,
            response,
        } = self.answered().await;

        let (parts, body) = response.into_parts();
        let record = client.record(&method, &path, uri, &parts, None);

        TestStream {
            method,
            path,
            parts,
            body: Some(body),
            received: BytesMut::new(),
            read: 0,
            observed: &client.observed,
            record,
        }
    }

    /// Sends the request, and returns once the response head arrives.
    async fn answered(mut self) -> Answered<'a, C> {
        #[cfg(feature = "server")]
        if let (RequestBody::StalledAfter(first), Some(length)) = (&self.body, self.content_length)
        {
            assert!(
                (first.len() as u64) < length,
                "{} {}: `stalled_after` sends {} octet(s) of a body declared {length} long, \
                 which then arrives whole rather than stalling",
                self.method,
                self.path,
                first.len()
            );
        }
        #[cfg(feature = "server")]
        let idle = self.client.request_body_idle_limit(self.body.is_stalled());
        #[cfg(not(feature = "server"))]
        let idle = None;
        let body = match (self.body, self.content_length) {
            (RequestBody::Whole(bytes), None) => Body::from_bytes(bytes),
            (body, remaining) => Body::from_body(Framed {
                body,
                remaining,
                failed: false,
            }),
        };
        let mut request = Request::new(body);
        *request.method_mut() = self.method.clone();
        let uri = self
            .path
            .parse::<Uri>()
            .unwrap_or_else(|_| panic!("`{}` is not a request target", self.path));
        *request.uri_mut() = uri.clone();
        if !self.cookies.is_empty() {
            let jar = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.headers.insert(
                crate::http::header::COOKIE,
                HeaderValue::from_str(&jar).expect("a cookie a test wrote"),
            );
        }

        *request.headers_mut() = std::mem::take(&mut self.headers);

        let response = self
            .client
            .transport
            .answer(request, self.peer, idle)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "{} {}: no response arrived: {error}",
                    self.method, self.path
                )
            });

        Answered {
            client: self.client,
            method: self.method,
            path: self.path,
            uri,
            response,
        }
    }

    /// Sends the request on a paused clock, and returns the response with
    /// how long it took on that clock.
    ///
    /// The clock is paused for the exchange and resumed after it, so a
    /// timeout of any length answers at once in real time and on its
    /// deadline on the paused clock: a test asserts the 408 came at its own
    /// deadline, and a refusal from the head at none. A timer fires on
    /// tokio's millisecond tick, so a deadline reads as up to a millisecond
    /// past it. That needs tokio's `test-util` feature, which this crate's
    /// `test-util` enables.
    ///
    /// No answer for ten seconds of wall time fails the test rather than
    /// hanging it: a body nothing bounds, or a handler waiting on something
    /// other than a timer. The guard is a thread on the real clock, since
    /// the paused clock would jump to any tokio timer.
    ///
    /// ```
    /// use kynos::{Router, test::TestClient};
    ///
    /// #[tokio::main(flavor = "current_thread")]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let client = TestClient::new(Router::<()>::new().build(())?);
    ///     let (reply, waited) = client.get("/").send_timed().await;
    ///     reply.assert_status(501);
    ///     assert!(waited.is_zero());
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics as [`send`](Self::send) does; outside a current-thread tokio
    /// runtime, and where the clock is already paused, as under
    /// `#[tokio::test(start_paused = true)]`, both of which
    /// `tokio::time::pause` refuses; when no response arrives within the
    /// guard; and on a client built [`connect`](TestClient::connect), whose
    /// real socket is ready on the real clock, not the paused one, so the
    /// clock would leap to the server's timers while a write is in flight.
    #[cfg(feature = "server")]
    pub async fn send_timed(self) -> (TestResponse, std::time::Duration) {
        let exchange = format!("{} {}", self.method, self.path);
        assert!(
            !matches!(self.client.transport, Transport::Connected { .. }),
            "{exchange}: a client built `connect` cannot time on a paused clock, \
             which leaps past a real socket's wait; use `through_server`"
        );
        crate::server::request_body::on_paused_clock(self.send(), SEND_TIMED_GUARD)
            .await
            .unwrap_or_else(|| {
                panic!(
                    "{exchange}: no response arrived within {SEND_TIMED_GUARD:?} of wall time; \
                     nothing bounds the wait on the paused clock"
                )
            })
    }
}

/// A request answered as far as its head, with the client that sent it.
struct Answered<'a, C> {
    client: &'a TestClient<C>,
    method: HttpMethod,
    path: String,
    uri: Uri,
    response: Response,
}

/// A response whose body a test reads as it arrives, from
/// [`TestRequest::send_streaming`].
///
/// [`next_chunk`](Self::next_chunk) and [`next_event`](Self::next_event)
/// read from one position, so each octet is returned once, by whichever
/// reads it first. Dropping the stream closes the body unread, as a client
/// that hangs up does: an operation's stream is dropped, and a connection
/// built [`through_server`](TestClient::through_server) or
/// [`connect`](TestClient::connect) closes.
///
/// Every panic a method here raises opens with the request and its status,
/// as [`TestResponse`]'s do.
#[derive(Debug)]
pub struct TestStream<'a> {
    method: HttpMethod,
    /// The target as the test wrote it.
    path: String,
    parts: ::http::response::Parts,
    /// `None` once the body has ended.
    body: Option<Body>,
    /// Every octet that has arrived, read or not, for [`Self::finish`].
    received: BytesMut,
    /// Where in `received` the octets nothing has read yet begin.
    read: usize,
    /// The client's record, where the body is filed once it ends.
    observed: &'a Mutex<Vec<Observed>>,
    /// This exchange's place in `observed`.
    record: usize,
}

impl TestStream<'_> {
    /// The status code.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.parts.status
    }

    /// The first value of a response header, as text.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.parts
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
    }

    /// The octets that arrived next and nothing has read yet: the next body
    /// frame, or what is left of one [`next_event`](Self::next_event) read
    /// part of. `None` once the body has ended.
    ///
    /// # Panics
    ///
    /// Panics when the body fails part-way.
    pub async fn next_chunk(&mut self) -> Option<Bytes> {
        if self.read == self.received.len() && !self.pull().await {
            return None;
        }
        let chunk = Bytes::copy_from_slice(&self.received[self.read..]);
        self.read = self.received.len();
        Some(chunk)
    }

    /// The next Server-Sent Event, parsed as [`TestResponse::events`] parses
    /// one, and skipping records that are only comments (keep-alives) as it
    /// does. Waits for as many frames as the event spans. `None` once the
    /// body has ended.
    ///
    /// Read to its end, a body yields the events `events` reads from the
    /// same octets, a record left unterminated at the end included.
    ///
    /// # Panics
    ///
    /// Panics when the body fails part-way, or a record is not UTF-8.
    pub async fn next_event(&mut self) -> Option<TestEvent> {
        let delimiter = EVENT_DELIMITER.as_bytes();
        loop {
            let unread = &self.received[self.read..];
            if let Some(at) = unread
                .windows(delimiter.len())
                .position(|window| window == delimiter)
            {
                let start = self.read;
                self.read += at + delimiter.len();
                if let Some(event) = parse_event(self.record_text(start, start + at)) {
                    return Some(event);
                }
            } else if !self.pull().await {
                // The body ended: what is left is its last record.
                let start = self.read;
                self.read = self.received.len();
                return parse_event(self.record_text(start, self.read));
            }
        }
    }

    /// Reads the rest of the body, and returns the whole response, every
    /// octet [`next_chunk`](Self::next_chunk) and
    /// [`next_event`](Self::next_event) already read included, as
    /// [`TestRequest::send`] would have.
    ///
    /// # Panics
    ///
    /// Panics when the body fails part-way.
    pub async fn finish(mut self) -> TestResponse {
        while self.pull().await {}
        let body = self.received.freeze();
        TestResponse {
            method: self.method,
            path: self.path,
            response: Response::from_parts(self.parts, Body::from_bytes(body.clone())),
            body,
        }
    }

    /// Waits for the next frame carrying octets and appends them, or files
    /// the whole body with the client and returns `false` once it ends.
    async fn pull(&mut self) -> bool {
        let Some(body) = &mut self.body else {
            return false;
        };
        loop {
            match body.frame().await {
                // A trailers frame, or an empty one, carries no octets.
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) if !data.is_empty() => {
                        self.received.extend_from_slice(&data);
                        return true;
                    }
                    _ => {}
                },
                Some(Err(error)) => panic!(
                    "{}: the response body failed part-way: {error}",
                    self.exchange()
                ),
                None => break,
            }
        }
        self.body = None;
        self.observed
            .lock()
            .expect("a test client whose recorder panicked cannot be written to")[self.record]
            .body = Some(Bytes::copy_from_slice(&self.received));
        false
    }

    /// The record between `start` and `end` of what arrived, as text.
    fn record_text(&self, start: usize, end: usize) -> &str {
        std::str::from_utf8(&self.received[start..end])
            .unwrap_or_else(|_| panic!("{}: an event record is not text", self.exchange()))
    }

    /// The request and the status it received, as every panic here opens.
    fn exchange(&self) -> String {
        format!(
            "{} {} -> {}",
            self.method,
            self.path,
            self.parts.status.as_u16()
        )
    }
}

/// How much wall time [`TestRequest::send_timed`] gives an exchange.
#[cfg(feature = "server")]
const SEND_TIMED_GUARD: std::time::Duration = std::time::Duration::from_secs(10);

/// A response received by a [`TestClient`], with the request it answers.
///
/// Every panic a method here raises opens with that request and the status it
/// received, as `GET /v1/items -> 404: `, so a failure in a loop over
/// operations names the one that failed.
#[derive(Debug)]
pub struct TestResponse {
    method: HttpMethod,
    /// The target as the test wrote it.
    path: String,
    response: Response,
    /// The drained body, kept so the response stays readable after assertions.
    body: Bytes,
}

impl TestResponse {
    /// The method the request was sent with.
    #[must_use]
    pub fn method(&self) -> &HttpMethod {
        &self.method
    }

    /// The request target as the test wrote it: the path given to the
    /// [`TestClient`], with whatever
    /// [`query_string`](TestRequest::query_string) appended.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The status code.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.response.status()
    }

    /// Deserializes the body as JSON.
    ///
    /// # Panics
    ///
    /// Panics when the body is not valid JSON for `T`.
    #[cfg(feature = "json")]
    #[must_use]
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "{}: the body is not valid JSON for `{}`: {error}\n{}",
                self.exchange(),
                std::any::type_name::<T>(),
                self.rendered_body()
            )
        })
    }

    /// The body as bytes.
    #[must_use]
    pub fn bytes(&self) -> &bytes::Bytes {
        &self.body
    }

    /// Asserts the status, given as a [`StatusCode`] or as its number.
    ///
    /// ```
    /// # async fn run(client: kynos::test::TestClient<()>) {
    /// use kynos::http::StatusCode;
    ///
    /// let reply = client.get("/items/7").send().await;
    /// reply.assert_status(StatusCode::NOT_FOUND).assert_status(404);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics, with the body included, when the status differs, and when a
    /// number is outside 100 to 999, which no status can be.
    pub fn assert_status(&self, expected: impl IntoStatus) -> &Self {
        let expected = expected.status().unwrap_or_else(|code| {
            panic!(
                "{}: `{code}` is not a status code, which is 100 to 999",
                self.exchange()
            )
        });
        let actual = self.status();
        assert!(
            actual == expected,
            "{}: expected {expected}, received {actual}\n{}",
            self.exchange(),
            self.rendered_body()
        );
        self
    }

    /// [`assert_status`](Self::assert_status), taking the response and
    /// handing it back, so a helper can send, assert and return in one
    /// expression.
    ///
    /// ```
    /// # async fn run(client: kynos::test::TestClient<()>) {
    /// use kynos::test::{IntoStatus, TestClient, TestResponse};
    ///
    /// async fn profile(client: &TestClient<()>, expected: impl IntoStatus) -> TestResponse {
    ///     client.get("/profile").send().await.expect_status(expected)
    /// }
    /// # let _ = profile(&client, 200).await;
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics as [`assert_status`](Self::assert_status) does.
    #[must_use = "a response nothing reads again is asserted with `assert_status`"]
    pub fn expect_status(self, expected: impl IntoStatus) -> Self {
        self.assert_status(expected);
        self
    }

    /// Asserts that the body is an RFC 9457 problem document of a given type.
    ///
    /// A problem document that omits `type` means `about:blank`, which RFC 9457
    /// section 3.1.1 makes the default rather than an absence.
    ///
    /// # Panics
    ///
    /// Panics when the body is not a problem document, or its `type` differs.
    pub fn assert_problem_type(&self, expected: &str) -> &Self {
        let document: Value = serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "{}: the body is not a problem document: {error}\n{}",
                self.exchange(),
                self.rendered_body()
            )
        });

        let actual = document
            .get("type")
            .map_or(Some("about:blank"), Value::as_str)
            .unwrap_or_else(|| {
                panic!(
                    "{}: the problem document's `type` is not a string\n{}",
                    self.exchange(),
                    self.rendered_body()
                )
            });

        assert!(
            actual == expected,
            "{}: expected problem type `{expected}`, received `{actual}`\n{}",
            self.exchange(),
            self.rendered_body()
        );
        self
    }

    /// Asserts that this response is an RFC 9457 problem of a given status and
    /// type, whole.
    ///
    /// Checks, in order: the status, as
    /// [`assert_status`](Self::assert_status) does; that `Content-Type` is
    /// `application/problem+json` (RFC 9457 section 6.1), its parameters aside;
    /// that the body's `type` is `type_uri`, an omitted one being
    /// `about:blank` (section 3.1.1); and that a `status` member, where the
    /// document carries one, names the status line, which section 3.1.2 binds a
    /// generator to.
    ///
    /// ```
    /// # async fn run(client: kynos::test::TestClient<()>) {
    /// client
    ///     .get("/items/7")
    ///     .send()
    ///     .await
    ///     .assert_problem(404, "https://errors.example.com/no-such-item");
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics when the status differs or names no status; when `Content-Type`
    /// is absent or another media type; when the body is not a problem
    /// document, as [`problem`](Self::problem) reads one; when its `type`
    /// differs; and when its `status` member disagrees with the status line.
    pub fn assert_problem(&self, status: impl IntoStatus, type_uri: &str) -> &Self {
        self.assert_status(status);

        match self
            .response
            .headers()
            .get(crate::http::header::CONTENT_TYPE)
        {
            Some(value) => {
                assert!(
                    crate::http::eq_ignore_ascii_case(
                        crate::http::media::split_essence(value.as_bytes()).0,
                        PROBLEM_JSON.as_bytes()
                    ),
                    "{}: expected `Content-Type: {PROBLEM_JSON}`, received `Content-Type: {}`\n{}",
                    self.exchange(),
                    value.as_bytes().escape_ascii(),
                    self.rendered_body()
                );
            }
            None => panic!(
                "{}: expected `Content-Type: {PROBLEM_JSON}`, and the response carries none\n{}",
                self.exchange(),
                self.rendered_body()
            ),
        }

        let problem = self.problem();
        assert!(
            problem.type_uri == type_uri,
            "{}: expected problem type `{type_uri}`, received `{}`\n{}",
            self.exchange(),
            problem.type_uri,
            self.rendered_body()
        );
        if let Some(member) = problem.status {
            assert!(
                member == self.status().as_u16(),
                "{}: the problem document's `status` is {member}, and the status line {}\n{}",
                self.exchange(),
                self.status().as_u16(),
                self.rendered_body()
            );
        }
        self
    }

    /// The body read as an RFC 9457 problem document.
    ///
    /// Reads the body alone: [`assert_problem`](Self::assert_problem) is what
    /// also holds the status and the media type.
    ///
    /// ```
    /// # async fn run(client: kynos::test::TestClient<()>) {
    /// use kynos::error::problem::PointerError;
    ///
    /// let problem = client.post("/items").send().await.problem();
    /// assert_eq!(problem.detail.as_deref(), Some("the body is not valid"));
    /// let errors: Option<Vec<PointerError>> = problem.extension("errors");
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// Panics when the body is not a JSON object, and when a member RFC 9457
    /// section 3.1 registers is not of the type it registers: a recipient
    /// ignores such a member, which in a test would hide the defect that
    /// produced it.
    #[must_use]
    pub fn problem(&self) -> TestProblem {
        let mut members = match serde_json::from_slice(&self.body) {
            Ok(Value::Object(members)) => members,
            Ok(_) => panic!(
                "{}: the body is not a problem document, which is a JSON object\n{}",
                self.exchange(),
                self.rendered_body()
            ),
            Err(error) => panic!(
                "{}: the body is not a problem document: {error}\n{}",
                self.exchange(),
                self.rendered_body()
            ),
        };

        let mut string = |name: &str| match members.remove(name) {
            None => None,
            Some(Value::String(value)) => Some(value),
            Some(_) => panic!(
                "{}: the problem document's `{name}` is not a string\n{}",
                self.exchange(),
                self.rendered_body()
            ),
        };
        let type_uri = string("type").unwrap_or_else(|| "about:blank".to_owned());
        let title = string("title");
        let detail = string("detail");
        let instance = string("instance");

        let status = members.remove("status").map(|value| {
            value
                .as_u64()
                .and_then(|number| u16::try_from(number).ok())
                .filter(|number| StatusCode::from_u16(*number).is_ok())
                .unwrap_or_else(|| {
                    panic!(
                        "{}: the problem document's `status` is not a status code\n{}",
                        self.exchange(),
                        self.rendered_body()
                    )
                })
        });

        TestProblem {
            type_uri,
            title,
            status,
            detail,
            instance,
            extensions: members,
        }
    }

    /// The body as text.
    ///
    /// # Panics
    ///
    /// Panics when the body is not UTF-8.
    #[must_use]
    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap_or_else(|_| {
            panic!(
                "{}: the body is not text\n{}",
                self.exchange(),
                self.rendered_body()
            );
        })
    }

    /// The first value of a response header, as text.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
    }

    /// Every value filed under `name`, in order.
    ///
    /// `Set-Cookie` is the field this exists for: HTTP forbids comma-joining
    /// it, so a response may carry several and [`header`](Self::header) reports
    /// only the first.
    #[must_use]
    pub fn headers(&self, name: &str) -> Vec<&str> {
        self.response
            .headers()
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect()
    }

    /// Every response header, as received.
    ///
    /// For comparing two responses field for field, or enumerating them,
    /// without naming each one. `Content-Length` is among them, as
    /// [`TestRequest::send`] says.
    ///
    /// ```
    /// # async fn run(client: kynos::test::TestClient<()>) {
    /// let unknown = client.get("/users/unknown").send().await;
    /// let wrong = client.get("/users/7").bearer("wrong").send().await;
    /// assert_eq!(unknown.header_map(), wrong.header_map());
    /// # }
    /// ```
    #[must_use]
    pub fn header_map(&self) -> &HeaderMap {
        self.response.headers()
    }

    /// The cookies this response set, by name.
    #[must_use]
    pub fn cookies(&self) -> Vec<(&str, &str)> {
        self.headers("set-cookie")
            .into_iter()
            .filter_map(|field| {
                let pair = field.split(';').next()?;
                let (name, value) = pair.split_once('=')?;
                Some((name.trim(), value.trim()))
            })
            .collect()
    }

    /// Asserts a response header has exactly this value.
    ///
    /// # Panics
    ///
    /// Panics when the field is absent or carries something else.
    pub fn assert_header(&self, name: &str, expected: &str) -> &Self {
        match self.header(name) {
            Some(actual) => assert!(
                actual == expected,
                "{}: expected `{name}: {expected}`, received `{name}: {actual}`",
                self.exchange()
            ),
            None => panic!("{}: `{name}` is not on the response", self.exchange()),
        }
        self
    }

    /// Asserts no field `name` is on the response.
    ///
    /// # Panics
    ///
    /// Panics when `name` is not a header name, which no response could carry
    /// and so would pass whatever was sent, and when the response carries the
    /// field, naming every value it carries.
    pub fn assert_no_header(&self, name: &str) -> &Self {
        let field = HeaderName::from_bytes(name.as_bytes())
            .unwrap_or_else(|_| panic!("{}: `{name}` is not a header name", self.exchange()));
        let values: Vec<String> = self
            .response
            .headers()
            .get_all(&field)
            .iter()
            .map(|value| format!("`{name}: {}`", value.as_bytes().escape_ascii()))
            .collect();
        assert!(
            values.is_empty(),
            "{}: expected no `{name}`, received {}",
            self.exchange(),
            values.join(", ")
        );
        self
    }

    /// Asserts a cookie was set with this value.
    ///
    /// # Panics
    ///
    /// Panics when no `Set-Cookie` names `name`, or it carries something else.
    pub fn assert_cookie(&self, name: &str, expected: &str) -> &Self {
        let cookies = self.cookies();
        match cookies.iter().find(|(set, _)| *set == name) {
            Some((_, actual)) => assert!(
                *actual == expected,
                "{}: expected cookie `{name}={expected}`, received `{name}={actual}`",
                self.exchange()
            ),
            None => panic!(
                "{}: no `Set-Cookie` names `{name}`; the response set {:?}",
                self.exchange(),
                cookies.iter().map(|(set, _)| *set).collect::<Vec<_>>()
            ),
        }
        self
    }

    /// Asserts this is a redirect to `location`.
    ///
    /// # Panics
    ///
    /// Panics when the status is not a redirect, or `Location` names something
    /// else.
    pub fn assert_redirect(&self, location: &str) -> &Self {
        assert!(
            self.status().is_redirection(),
            "{}: expected a redirect, received {}",
            self.exchange(),
            self.status()
        );
        self.assert_header("location", location);
        self
    }

    /// Asserts this is a 206 enclosing exactly `range` of `complete_length`.
    ///
    /// Checks the field *and* that the body length matches it (RFC 9110
    /// section 14.4).
    ///
    /// # Panics
    ///
    /// Panics when `last` precedes `first`, which names no octets; when the
    /// status is not 206, the field is absent or names another span, or the
    /// body is not the length the field claims.
    pub fn assert_part(&self, first: u64, last: u64, complete_length: u64) -> &Self {
        assert!(
            first <= last,
            "{}: `bytes {first}-{last}` names no octets: its last position precedes its first",
            self.exchange()
        );
        assert!(
            self.status() == StatusCode::PARTIAL_CONTENT,
            "{}: expected 206, received {}",
            self.exchange(),
            self.status()
        );

        let expected = format!("bytes {first}-{last}/{complete_length}");
        self.assert_header("content-range", &expected);

        // Wider than `u64`, which `0-18446744073709551615` overflows by one.
        let enclosed = u128::from(last - first) + 1;
        assert!(
            self.body.len() as u128 == enclosed,
            "{}: `Content-Range` names {enclosed} octet(s) and the body carries {}",
            self.exchange(),
            self.body.len()
        );
        self
    }

    /// The Server-Sent Events this response carries, parsed.
    ///
    /// The body is already drained, so the feed under test must be bounded;
    /// [`TestRequest::send_streaming`] reads one that is not as it arrives.
    /// Comment lines (keep-alives) are dropped, as a client ignores them.
    #[must_use]
    pub fn events(&self) -> Vec<TestEvent> {
        self.text()
            .split(EVENT_DELIMITER)
            .filter_map(parse_event)
            .collect()
    }

    /// The underlying response.
    #[must_use]
    pub fn into_inner(self) -> Response {
        self.response
    }

    /// The request and the status it received, as every panic here opens.
    fn exchange(&self) -> String {
        format!(
            "{} {} -> {}",
            self.method,
            self.path,
            self.status().as_u16()
        )
    }

    /// The body as an assertion message shows it.
    fn rendered_body(&self) -> String {
        match std::str::from_utf8(&self.body) {
            Ok(text) => format!("body: {text}"),
            Err(_) => format!("body: {} bytes, not UTF-8", self.body.len()),
        }
    }
}

/// A status [`TestResponse::assert_status`] expects: a [`StatusCode`], or its
/// number, which a table of expected outcomes holds more naturally.
///
/// Sealed: implemented for [`StatusCode`] and [`u16`], so an integer literal
/// is inferred as a `u16` without a suffix.
pub trait IntoStatus: sealed::Sealed {
    /// The status this names.
    ///
    /// # Panics
    ///
    /// Panics when a number is outside 100 to 999, which no status can be.
    fn into_status(self) -> StatusCode;
}

mod sealed {
    use crate::http::StatusCode;

    pub trait Sealed: Sized {
        /// The status, or the number that names none.
        fn status(self) -> Result<StatusCode, u16>;
    }
}

impl sealed::Sealed for StatusCode {
    fn status(self) -> Result<StatusCode, u16> {
        Ok(self)
    }
}

impl sealed::Sealed for u16 {
    fn status(self) -> Result<StatusCode, u16> {
        StatusCode::from_u16(self).map_err(|_| self)
    }
}

impl IntoStatus for StatusCode {
    fn into_status(self) -> StatusCode {
        self
    }
}

impl IntoStatus for u16 {
    fn into_status(self) -> StatusCode {
        sealed::Sealed::status(self)
            .unwrap_or_else(|code| panic!("`{code}` is not a status code, which is 100 to 999"))
    }
}

/// A header field a test wrote as text.
///
/// # Panics
///
/// Panics when `name` or `value` is not one HTTP can carry.
fn field(name: &str, value: &str) -> (HeaderName, HeaderValue) {
    let name = HeaderName::from_bytes(name.as_bytes())
        .unwrap_or_else(|_| panic!("`{name}` is not a header name"));
    let value =
        HeaderValue::from_str(value).unwrap_or_else(|_| panic!("`{value}` is not a header value"));
    (name, value)
}

/// `n responses`, or `1 response`, for an assertion message that counts them.
fn responses(count: usize) -> String {
    if count == 1 {
        "1 response".to_owned()
    } else {
        format!("{count} responses")
    }
}

/// What ends one Server-Sent Event record: the blank line after its fields.
const EVENT_DELIMITER: &str = "\n\n";

/// The event one record between [`EVENT_DELIMITER`]s carries, or `None` for
/// a record that is blank or only comments: a keep-alive, not an event.
fn parse_event(record: &str) -> Option<TestEvent> {
    let mut event = TestEvent::default();
    let mut data: Vec<&str> = Vec::new();
    let mut carried = false;

    for line in record.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.strip_prefix(' ').unwrap_or(value);

        match name {
            // Comments (empty name) and unknown fields fall to `_`.
            "data" => {
                data.push(value);
                carried = true;
            }
            "id" => {
                event.id = Some(value.to_owned());
                carried = true;
            }
            "event" => {
                event.event = Some(value.to_owned());
                carried = true;
            }
            "retry" => {
                event.retry = value.parse().ok();
                carried = true;
            }
            _ => {}
        }
    }

    carried.then(|| {
        event.data = data.join("\n");
        event
    })
}

/// One Server-Sent Event, parsed, as a test reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestEvent {
    /// The `data` value, with a multi-line one rejoined.
    pub data: String,
    /// The `event` name, which a client's listener matches on.
    pub event: Option<String>,
    /// The `id`, which a client returns as `Last-Event-ID` on reconnect.
    pub id: Option<String>,
    /// The `retry` advice, in milliseconds.
    pub retry: Option<u64>,
}

impl TestEvent {
    /// The `data` value parsed as JSON.
    ///
    /// # Panics
    ///
    /// Panics when `data` is not the JSON `T`.
    #[cfg(feature = "json")]
    #[must_use]
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> T {
        serde_json::from_str(&self.data)
            .unwrap_or_else(|error| panic!("`{}` is not the expected JSON: {error}", self.data))
    }
}

/// An RFC 9457 problem document, as [`TestResponse::problem`] read it.
///
/// The members section 3.1 registers are typed; every other member is an
/// extension, read by [`extension`](Self::extension).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TestProblem {
    /// The `type` URI, `about:blank` where the document omits it (RFC 9457
    /// section 3.1.1).
    pub type_uri: String,
    /// The `title`.
    pub title: Option<String>,
    /// The `status` member, which is not necessarily the status line.
    pub status: Option<u16>,
    /// The `detail`.
    pub detail: Option<String>,
    /// The `instance` URI.
    pub instance: Option<String>,
    /// Every member the five above are not.
    extensions: Map<String, Value>,
}

impl TestProblem {
    /// The extension member `name`, deserialized; `None` when the document
    /// does not carry it.
    ///
    /// # Panics
    ///
    /// Panics when `name` is one of the five registered members, which are
    /// read from their fields and would otherwise read as absent here, and
    /// when the member is present but not a `T`.
    #[must_use]
    pub fn extension<T: serde::de::DeserializeOwned>(&self, name: &str) -> Option<T> {
        assert!(
            !["type", "title", "status", "detail", "instance"].contains(&name),
            "`{name}` is a member RFC 9457 registers, not an extension; read its field"
        );
        let value = self.extensions.get(name)?;
        Some(T::deserialize(value).unwrap_or_else(|error| {
            panic!(
                "the problem document's `{name}` is not a `{}`: {error}\n{name}: {value}",
                std::any::type_name::<T>()
            )
        }))
    }
}
