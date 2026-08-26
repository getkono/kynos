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

    fn request(&self, method: HttpMethod, path: &str) -> TestRequest<'_, C> {
        TestRequest {
            client: self,
            method,
            path: path.to_owned(),
            headers: HeaderMap::new(),
            body: Bytes::new(),
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
                let operation = document.paths.0.get(template)?.operation(method)?;
                let (key, _) = declared_response(&operation.responses, record.status.as_u16())?;
                Some((template, method, key))
            })
            .collect();

        let mut missing = Vec::new();
        for (template, item) in &document.paths.0 {
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

    /// Sends the request.
    ///
    /// # Panics
    ///
    /// Panics when the path is not a request target, or when the response body
    /// fails part-way through — neither of which a service driven in-process
    /// can do to a test that spelled its path correctly.
    pub async fn send(self) -> TestResponse {
        let mut request = Request::new(Body::from_bytes(self.body));
        *request.method_mut() = self.method.clone();
        *request.uri_mut() = self
            .path
            .parse::<Uri>()
            .unwrap_or_else(|_| panic!("`{}` is not a request target", self.path));
        *request.headers_mut() = self.headers;

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
