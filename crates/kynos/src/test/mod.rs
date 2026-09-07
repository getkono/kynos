//! Driving a router in-process, without a socket.
//!
//! Beyond the usual convenience, this module offers something the description
//! makes possible and nothing else provides: it can check that the responses a
//! test actually observed match what the description promises. A test suite
//! that exercises every operation therefore also proves the document is
//! truthful — see [`TestClient::assert_conformance`].

// Private: it declares no item a canonical path could point at, only the
// checks the two assertions below are written in terms of.
mod conformance;

use std::{collections::BTreeSet, sync::Mutex};

use bytes::Bytes;
use http_body_util::BodyExt;
use kynos_openapi::Method;
use serde_json::Value;

use crate::{
    http::{
        HeaderMap, HeaderName, HeaderValue, Method as HttpMethod, Request, Response, StatusCode,
        Uri, body::Body,
    },
    router::service::Service,
    test::conformance::{conformance, declared_keys, declared_response, matched_template},
};

/// One response, as it was received.
///
/// The recorded request is the *concrete* path rather than the template that
/// matched it: the description is the only authority on templates, so the match
/// is redone against it when an assertion runs rather than being cached here.
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
    /// Interior mutability because [`TestRequest::send`] borrows the client
    /// shared: a test holds one client and chains requests off it.
    observed: Mutex<Vec<Observed>>,
}

impl<C> TestClient<C> {
    /// Wraps a built service.
    #[must_use]
    pub fn new(service: Service<C>) -> Self {
        Self {
            service,
            observed: Mutex::new(Vec::new()),
        }
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
    ///
    /// Routable through `#[kynos::head]` and, until now, untestable — which is
    /// the shape of every method below: the router accepts them and nothing
    /// here could send one.
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
    /// Registered by `#[kynos::query]`, and not one `http::Method` names, so it
    /// is built from the token.
    #[must_use]
    pub fn query(&self, path: &str) -> TestRequest<'_, C> {
        self.request(
            HttpMethod::from_bytes(b"QUERY").expect("`QUERY` is a method token"),
            path,
        )
    }

    /// Begins a request with any method.
    ///
    /// The escape hatch for a method Kynos routes but does not name, so a test
    /// is never blocked on this type growing a verb.
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
            body: Bytes::new(),
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
    /// A response declaring *no* representation is checked too. Declaring
    /// nothing is a claim about the exchange rather than the absence of one, so
    /// a body or a `Content-Type` arriving under it is reported.
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
    /// that the description promises and no test has ever produced.
    ///
    /// # Panics
    ///
    /// Panics listing every declared response that was never seen.
    pub fn assert_declared_responses_covered(&self) {
        let document = self.service.openapi();

        let exercised: BTreeSet<(&str, Method, String)> = self
            .recorded()
            .iter()
            .filter_map(|record| {
                let template = matched_template(document, &record.path)?;
                let method = Method::from_wire_str(record.method.as_str())?;
                let operation = document.paths.items.get(template)?.operation(method)?;
                let (key, _) = declared_response(&operation.responses, record.status.as_u16())?;
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
}

/// A request under construction.
#[derive(Debug)]
pub struct TestRequest<'a, C> {
    client: &'a TestClient<C>,
    method: HttpMethod,
    path: String,
    headers: HeaderMap,
    body: Bytes,
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
    /// Panics when `name` or `value` is not one HTTP can carry. A test writes
    /// both as literals, so a malformed one is a mistake in the test rather
    /// than a condition to handle.
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
    #[cfg(feature = "json")]
    #[must_use]
    pub fn json<T: serde::Serialize>(mut self, body: &T) -> Self {
        self.body = Bytes::from(serde_json::to_vec(body).expect("a serializable request body"));
        self.headers.insert(
            crate::http::header::CONTENT_TYPE,
            HeaderValue::from_static(kynos_openapi::model::body::mime_names::APPLICATION_JSON),
        );
        self
    }

    /// Adds a query string to the target, encoded from a serializable value.
    ///
    /// Named for the part of the target it writes, because [`TestClient::query`]
    /// one link up the chain begins a `QUERY` request: two `query` methods on
    /// one expression would read as the same call.
    ///
    /// Appends to whatever the path already carries, so a test can name the
    /// stable part of a target once and vary the rest.
    ///
    /// # Panics
    ///
    /// Panics when `value` cannot be a query string, which a test writing a
    /// struct of scalars cannot cause.
    #[cfg(feature = "form")]
    #[must_use]
    pub fn query_string<T: serde::Serialize>(mut self, value: &T) -> Self {
        let encoded = serde_urlencoded::to_string(value).expect("a serializable query");
        if !encoded.is_empty() {
            let separator = if self.path.contains('?') { '&' } else { '?' };
            self.path.push(separator);
            self.path.push_str(&encoded);
        }
        self
    }

    /// Sends a cookie.
    ///
    /// Accumulated rather than set, because RFC 6265 section 5.4 puts every
    /// cookie in *one* `Cookie` field separated by `; ` — a client that sent
    /// two fields would be testing a shape no browser produces.
    #[must_use]
    pub fn cookie(mut self, name: &str, value: &str) -> Self {
        self.cookies.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Says who the request came from.
    ///
    /// Without one, a service reading a peer address sees the in-process
    /// default — which is right for most tests and wrong for exactly the ones
    /// that care: a rate limiter keyed by client, or a trusted-proxy policy.
    #[must_use]
    pub fn peer(mut self, address: std::net::SocketAddr) -> Self {
        self.peer = Some(address);
        self
    }

    /// Sets a raw body, and the media type it is in.
    #[must_use]
    pub fn body(mut self, media_type: &str, bytes: impl Into<Bytes>) -> Self {
        self.body = bytes.into();
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
        let encoded = serde_urlencoded::to_string(body).expect("a serializable form body");
        self.body(
            "application/x-www-form-urlencoded",
            Bytes::from(encoded.into_bytes()),
        )
    }

    /// Sends the request.
    ///
    /// # Panics
    ///
    /// Panics when the path is not a request target, or when the response body
    /// fails part-way through — neither of which a service driven in-process
    /// can do to a test that spelled its path correctly.
    pub async fn send(mut self) -> TestResponse {
        let mut request = Request::new(Body::from_bytes(self.body));
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
            // The same extension the server inserts per connection, so a
            // service reading one cannot tell a test from a socket.
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
    /// Kept beside the response because the body was already drained to record
    /// it: a `TestResponse` stays readable after every assertion, and
    /// [`into_inner`](TestResponse::into_inner) still hands back a whole
    /// response.
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
    /// Panics with the body included, since a failing assertion is nearly
    /// always explained by it.
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
    /// Panics when the body is not UTF-8, which a test asserting text has
    /// already decided it is.
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
    /// Checks the field *and* the body, because the pair is what a range
    /// response is: a `Content-Range` naming octets the body does not carry
    /// produces a field RFC 9110 section 14.4 tells a recipient never to
    /// recombine, and either half alone passes while the response is wrong.
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
    /// The body is already drained, so this parses what arrived rather than
    /// waiting for more — which is what makes asserting a *finite* number of
    /// events possible without the stream closing first. A handler under test
    /// sends a bounded feed and this reads it; an endless one is driven over a
    /// real socket instead, where `tests/sse.rs` reads for a deadline.
    ///
    /// Comment lines are dropped: the protocol requires a client to ignore
    /// them, which is exactly what makes them usable as a keep-alive, so a test
    /// counting events must not count heartbeats.
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
                    // `: comment` -- ignored by every client, and by this.
                    let Some((name, value)) = line.split_once(':') else {
                        continue;
                    };
                    let value = value.strip_prefix(' ').unwrap_or(value);

                    match name {
                        // An empty name is the comment form, `: text`, which a
                        // client ignores and so does this -- along with any
                        // field the protocol grows that a test cannot assert
                        // on yet.
                        "data" => {
                            data.push(value);
                            carried = true;
                        }
                        "id" => {
                            event.id = Some(value.to_owned());
                            carried = true;
                        }
                        "event" => {
                            event.name = Some(value.to_owned());
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
                    // A value spanning several `data` lines is rejoined with the
                    // newlines the encoder split it on.
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

/// One Server-Sent Event, as a test reads it.
///
/// The parsed form rather than the wire form: a test asserting what a client
/// receives should not be re-implementing the framing to do it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestEvent {
    /// The `data` value, with a multi-line one rejoined.
    pub data: String,
    /// The `event` name, which a client's listener matches on.
    pub name: Option<String>,
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
    /// Panics when `data` is not the JSON `T`, which is the assertion a test
    /// wanted to make anyway.
    #[cfg(feature = "json")]
    #[must_use]
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> T {
        serde_json::from_str(&self.data)
            .unwrap_or_else(|error| panic!("`{}` is not the expected JSON: {error}", self.data))
    }
}
