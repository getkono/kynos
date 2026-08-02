//! Driving a router in-process, without a socket.
//!
//! Beyond the usual convenience, this module offers something the description
//! makes possible and nothing else provides: it can check that the responses a
//! test actually observed match what the description promises. A test suite
//! that exercises every operation therefore also proves the document is
//! truthful — see [`TestClient::assert_conformance`].

use crate::{
    http::{Response, StatusCode},
    router::Service,
};

/// Sends requests to a [`Service`] directly.
#[derive(Debug)]
pub struct TestClient<C> {
    _private: std::marker::PhantomData<C>,
}

impl<C> TestClient<C> {
    /// Wraps a built service.
    #[must_use]
    pub fn new(service: Service<C>) -> Self {
        let _ = service;
        todo!()
    }

    /// Begins a `GET` request.
    #[must_use]
    pub fn get(&self, path: &str) -> TestRequest<'_, C> {
        let _ = path;
        todo!()
    }

    /// Begins a `POST` request.
    #[must_use]
    pub fn post(&self, path: &str) -> TestRequest<'_, C> {
        let _ = path;
        todo!()
    }

    /// Begins a `PUT` request.
    #[must_use]
    pub fn put(&self, path: &str) -> TestRequest<'_, C> {
        let _ = path;
        todo!()
    }

    /// Begins a `PATCH` request.
    #[must_use]
    pub fn patch(&self, path: &str) -> TestRequest<'_, C> {
        let _ = path;
        todo!()
    }

    /// Begins a `DELETE` request.
    #[must_use]
    pub fn delete(&self, path: &str) -> TestRequest<'_, C> {
        let _ = path;
        todo!()
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
        todo!()
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
        todo!()
    }
}

/// A request under construction.
#[derive(Debug)]
pub struct TestRequest<'a, C> {
    _private: std::marker::PhantomData<&'a C>,
}

impl<C> TestRequest<'_, C> {
    /// Sets a header.
    #[must_use]
    pub fn header(self, name: &str, value: &str) -> Self {
        let _ = (name, value);
        todo!()
    }

    /// Sets a JSON body.
    #[must_use]
    pub fn json<T: serde::Serialize>(self, body: &T) -> Self {
        let _ = body;
        todo!()
    }

    /// Sends the request.
    pub async fn send(self) -> TestResponse {
        todo!()
    }
}

/// A response received by a [`TestClient`].
#[derive(Debug)]
pub struct TestResponse {
    _private: (),
}

impl TestResponse {
    /// The status code.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        todo!()
    }

    /// Deserializes the body as JSON.
    ///
    /// # Panics
    ///
    /// Panics when the body is not valid JSON for `T`.
    #[must_use]
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> T {
        todo!()
    }

    /// The body as bytes.
    #[must_use]
    pub fn bytes(&self) -> &bytes::Bytes {
        todo!()
    }

    /// Asserts the status.
    ///
    /// # Panics
    ///
    /// Panics with the body included, since a failing assertion is nearly
    /// always explained by it.
    pub fn assert_status(&self, expected: StatusCode) -> &Self {
        let _ = expected;
        todo!()
    }

    /// Asserts that the body is an RFC 9457 problem document of a given type.
    ///
    /// # Panics
    ///
    /// Panics when the body is not a problem document, or its `type` differs.
    pub fn assert_problem_type(&self, expected: &str) -> &Self {
        let _ = expected;
        todo!()
    }

    /// The underlying response.
    #[must_use]
    pub fn into_inner(self) -> Response {
        todo!()
    }
}
