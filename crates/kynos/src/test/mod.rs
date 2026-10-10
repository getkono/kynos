//! Driving a router in-process, without a socket.
//!
//! The client also checks that the responses a test observed match what the
//! description promises, so a suite exercising every operation proves the
//! document truthful — see [`TestClient::assert_conformance`]. Where the
//! declared responses need several differently built services to produce, a
//! [`Coverage`] shared between their clients checks them together.

mod conformance;

use std::{
    collections::BTreeSet,
    fmt,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::BodyExt;
use kynos_openapi::{Document, Method};
use serde_json::Value;

use crate::{
    http::{
        HeaderMap, HeaderName, HeaderValue, Method as HttpMethod, Request, Response, StatusCode,
        Uri,
        body::{Body, BoxError},
    },
    router::service::Service,
    test::conformance::{conformance, declared_keys, declared_response, matched_template},
};

/// One response, as it was received, with the concrete path; the template is
/// matched against the description when an assertion runs.
#[derive(Debug)]
struct Observed {
    method: HttpMethod,
    path: String,
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

/// Sends requests to a [`Service`] directly.
#[derive(Debug)]
pub struct TestClient<C> {
    service: Service<C>,
    /// Behind a lock because [`TestRequest::send`] borrows the client shared.
    observed: Mutex<Vec<Observed>>,
    /// Where every exchange is also recorded, when the client shares one.
    coverage: Option<Arc<Mutex<Vec<Exchange>>>>,
}

impl<C> TestClient<C> {
    /// Wraps a built service.
    #[must_use]
    pub fn new(service: Service<C>) -> Self {
        Self {
            service,
            observed: Mutex::new(Vec::new()),
            coverage: None,
        }
    }

    /// Records every response this client receives into `coverage` as well.
    ///
    /// The client's own assertions still read only what it received; the
    /// shared record is read by [`Coverage::assert_declared_responses_covered`].
    /// A second call replaces the first.
    #[must_use]
    pub fn with_coverage(mut self, coverage: &Coverage) -> Self {
        self.coverage = Some(Arc::clone(&coverage.exchanges));
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
    /// The method `#[kynos::query]` registers.
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
            headers: HeaderMap::new(),
            body: RequestBody::Whole(Bytes::new()),
            content_length: None,
            peer: None,
            cookies: Vec::new(),
        }
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
        let document = self.service.openapi();
        let mut failures = Vec::new();

        for record in self.recorded().iter() {
            for reason in conformance(document, record) {
                failures.push(format!(
                    "  {} {} -> {}: {reason}",
                    record.method,
                    record.path,
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
            self.service.openapi(),
            recorded
                .iter()
                .map(|record| (&record.method, record.path.as_str(), record.status)),
        );
    }
}

/// The responses any number of [`TestClient`]s received, checked together
/// against one description.
///
/// A description may declare responses no single service can produce: one
/// built with authentication and one without, or with an optional subsystem
/// present and absent. Each of those clients is built
/// [`with_coverage`](TestClient::with_coverage) over the same `Coverage`, and
/// the declared responses are asserted once, after all of them ran, against
/// the description that declares them all.
#[derive(Debug, Default)]
pub struct Coverage {
    exchanges: Arc<Mutex<Vec<Exchange>>>,
}

impl Coverage {
    /// A record no client has written to yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        let exchanges = self
            .exchanges
            .lock()
            .expect("a coverage record whose recorder panicked cannot be asserted on");
        assert_covered(
            document,
            exchanges
                .iter()
                .map(|exchange| (&exchange.method, exchange.path.as_str(), exchange.status)),
        );
    }
}

/// What a [`Coverage`] keeps of one exchange: enough to match it against a
/// description later.
#[derive(Debug)]
struct Exchange {
    method: HttpMethod,
    path: String,
    status: StatusCode,
}

/// Asserts that every response `document` declares is among `received`.
fn assert_covered<'r>(
    document: &Document,
    received: impl Iterator<Item = (&'r HttpMethod, &'r str, StatusCode)>,
) {
    let exercised: BTreeSet<(&str, Method, String)> = received
        .filter_map(|(method, path, status)| {
            let template = matched_template(document, path)?;
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

/// What a [`TestRequest`] sends as its body.
enum RequestBody {
    /// Every octet at once; empty once sent.
    Whole(Bytes),
    /// Chunks as a stream yields them.
    Streamed(Pin<Box<dyn futures_core::Stream<Item = Bytes> + Send>>),
}

impl RequestBody {
    /// The next chunk, empty or not.
    fn poll_chunk(&mut self, context: &mut Context<'_>) -> Poll<Option<Bytes>> {
        match self {
            Self::Whole(bytes) if bytes.is_empty() => Poll::Ready(None),
            Self::Whole(bytes) => Poll::Ready(Some(std::mem::take(bytes))),
            Self::Streamed(chunks) => chunks.as_mut().poll_next(context),
        }
    }
}

impl fmt::Debug for RequestBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Whole(bytes) => formatter.debug_tuple("Whole").field(bytes).finish(),
            Self::Streamed(_) => formatter.debug_tuple("Streamed").finish_non_exhaustive(),
        }
    }
}

/// A request body read as hyper reads one: frame by frame, and under a
/// declared length exactly that many octets end it, nothing past them is read,
/// and an end before them fails the read.
struct Framed {
    body: RequestBody,
    /// Octets still owed; `None` when no length was declared.
    remaining: Option<u64>,
    /// Set once a short end has been reported.
    failed: bool,
}

impl HttpBody for Framed {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if this.failed || this.remaining == Some(0) {
            return Poll::Ready(None);
        }

        loop {
            let Some(mut chunk) = std::task::ready!(this.body.poll_chunk(context)) else {
                let Some(owed) = this.remaining else {
                    return Poll::Ready(None);
                };
                this.failed = true;
                let short = std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!(
                        "the body ended {owed} octet(s) short of its declared `Content-Length`"
                    ),
                );
                return Poll::Ready(Some(Err(short.into())));
            };

            // A server yields no empty data frame.
            if chunk.is_empty() {
                continue;
            }
            if let Some(remaining) = &mut this.remaining {
                chunk.truncate(usize::try_from(*remaining).unwrap_or(usize::MAX));
                *remaining -= chunk.len() as u64;
            }
            return Poll::Ready(Some(Ok(Frame::data(chunk))));
        }
    }

    // Never after a short end, which is a failure rather than an end.
    fn is_end_stream(&self) -> bool {
        self.remaining == Some(0)
    }

    fn size_hint(&self) -> SizeHint {
        self.remaining
            .map_or_else(SizeHint::default, SizeHint::with_exact)
    }
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

impl<C> TestRequest<'_, C> {
    /// Sets a header.
    ///
    /// # Panics
    ///
    /// Panics when `name` or `value` is not one HTTP can carry.
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        let name = HeaderName::from_bytes(name.as_bytes())
            .unwrap_or_else(|_| panic!("`{name}` is not a header name"));
        let value = HeaderValue::from_str(value)
            .unwrap_or_else(|_| panic!("`{value}` is not a header value"));
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
    #[must_use]
    pub fn cookie(mut self, name: &str, value: &str) -> Self {
        self.cookies.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Says who the request came from.
    ///
    /// Without one, a service reading a peer address sees the in-process
    /// default; set it to test a rate limiter keyed by client, or a
    /// trusted-proxy policy.
    #[must_use]
    pub fn peer(mut self, address: std::net::SocketAddr) -> Self {
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
    /// never yields again is a stalled one. Without
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
    /// # Panics
    ///
    /// Panics when the path is not a request target, or when the response body
    /// fails part-way through.
    pub async fn send(mut self) -> TestResponse {
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
        *request.uri_mut() = self
            .path
            .parse::<Uri>()
            .unwrap_or_else(|_| panic!("`{}` is not a request target", self.path));
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

        if let Some(peer) = self.peer {
            // The same extension the server inserts per connection.
            request
                .extensions_mut()
                .insert(crate::extract::connection::Connection::from_peer(
                    peer,
                    std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                ));
        }

        let response = self.client.service.call(request).await;

        let (parts, body) = response.into_parts();
        let bytes = body
            .collect()
            .await
            .expect("a response body driven in-process cannot fail")
            .to_bytes();

        if let Some(coverage) = &self.client.coverage {
            coverage
                .lock()
                .expect("a coverage record whose recorder panicked cannot be written to")
                .push(Exchange {
                    method: self.method.clone(),
                    path: self.path.clone(),
                    status: parts.status,
                });
        }

        self.client.recorded().push(Observed {
            method: self.method,
            path: self.path,
            status: parts.status,
            headers: parts.headers.clone(),
            body: bytes.clone(),
        });

        TestResponse {
            response: Response::from_parts(parts, Body::from_bytes(bytes.clone())),
            body: bytes,
        }
    }
}

/// A response received by a [`TestClient`].
#[derive(Debug)]
pub struct TestResponse {
    response: Response,
    /// The drained body, kept so the response stays readable after assertions.
    body: Bytes,
}

impl TestResponse {
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
                "the body is not valid JSON for `{}`: {error}\n{}",
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

    /// Asserts the status.
    ///
    /// # Panics
    ///
    /// Panics, with the body included, when the status differs.
    pub fn assert_status(&self, expected: StatusCode) -> &Self {
        let actual = self.status();
        assert!(
            actual == expected,
            "expected {expected}, received {actual}\n{}",
            self.rendered_body()
        );
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
                "the body is not a problem document: {error}\n{}",
                self.rendered_body()
            )
        });

        let actual = document
            .get("type")
            .map_or(Some("about:blank"), Value::as_str)
            .unwrap_or_else(|| {
                panic!(
                    "the problem document's `type` is not a string\n{}",
                    self.rendered_body()
                )
            });

        assert!(
            actual == expected,
            "expected problem type `{expected}`, received `{actual}`\n{}",
            self.rendered_body()
        );
        self
    }

    /// The body as text.
    ///
    /// # Panics
    ///
    /// Panics when the body is not UTF-8.
    #[must_use]
    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap_or_else(|_| {
            panic!("the body is not text\n{}", self.rendered_body());
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
                "expected `{name}: {expected}`, received `{name}: {actual}`"
            ),
            None => panic!("`{name}` is not on the response"),
        }
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
                "expected cookie `{name}={expected}`, received `{name}={actual}`"
            ),
            None => panic!(
                "no `Set-Cookie` names `{name}`; the response set {:?}",
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
            "expected a redirect, received {}",
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
    /// Panics when the status is not 206, the field is absent or names another
    /// span, or the body is not the length the field claims.
    pub fn assert_part(&self, first: u64, last: u64, complete_length: u64) -> &Self {
        assert!(
            self.status() == StatusCode::PARTIAL_CONTENT,
            "expected 206, received {}",
            self.status()
        );

        let expected = format!("bytes {first}-{last}/{complete_length}");
        self.assert_header("content-range", &expected);

        let enclosed = last - first + 1;
        assert!(
            self.body.len() as u64 == enclosed,
            "`Content-Range` names {enclosed} octet(s) and the body carries {}",
            self.body.len()
        );
        self
    }

    /// The Server-Sent Events this response carries, parsed.
    ///
    /// The body is already drained, so the feed under test must be bounded.
    /// Comment lines (keep-alives) are dropped, as a client ignores them.
    #[must_use]
    pub fn events(&self) -> Vec<TestEvent> {
        self.text()
            .split("\n\n")
            .filter(|record| !record.trim().is_empty())
            .filter_map(|record| {
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

                // A record that was only comments is a keep-alive, not an event.
                carried.then(|| {
                    event.data = data.join("\n");
                    event
                })
            })
            .collect()
    }

    /// The underlying response.
    #[must_use]
    pub fn into_inner(self) -> Response {
        self.response
    }

    /// The body as an assertion message shows it.
    fn rendered_body(&self) -> String {
        match std::str::from_utf8(&self.body) {
            Ok(text) => format!("body: {text}"),
            Err(_) => format!("body: {} bytes, not UTF-8", self.body.len()),
        }
    }
}

/// `n responses`, or `1 response`, for an assertion message that counts them.
fn responses(count: usize) -> String {
    if count == 1 {
        "1 response".to_owned()
    } else {
        format!("{count} responses")
    }
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
