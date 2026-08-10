//! Cross-origin resource sharing.
//!
//! Out-of-document: a preflight `OPTIONS` is a browser protocol detail, not an
//! operation of the API, so it contributes nothing. Use
//! [`Cors::document_response_headers`] when the CORS response headers are part
//! of what you want clients to know about.

use crate::{
    http,
    middleware::{Interceptor, Next, contribution::OperationContribution},
    router::operation::Route,
};

/// CORS configuration.
#[derive(Clone, Debug, Default)]
pub struct Cors {
    _private: (),
}

impl Cors {
    /// A configuration permitting nothing, to be widened deliberately.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }

    /// Permits these origins.
    #[must_use]
    pub fn allow_origins(self, origins: &'static [&'static str]) -> Self {
        let _ = origins;
        todo!()
    }

    /// Permits any origin.
    ///
    /// Incompatible with [`allow_credentials`](Cors::allow_credentials): the
    /// CORS protocol forbids `Access-Control-Allow-Origin: *` on a credentialed
    /// response, so selecting both is rejected when the router is built rather
    /// than producing a header browsers will refuse.
    #[must_use]
    pub fn allow_any_origin(self) -> Self {
        todo!()
    }

    /// Overrides the methods advertised on preflight.
    ///
    /// By default these are derived from the operations declared on the matched
    /// path, so what preflight advertises and what the description promises
    /// cannot disagree. Overriding is for a deployment that fronts routes Kynos
    /// does not serve.
    #[must_use]
    pub fn allow_methods(self, methods: &'static [kynos_openapi::Method]) -> Self {
        let _ = methods;
        todo!()
    }

    /// Permits these request headers.
    #[must_use]
    pub fn allow_headers(self, names: &'static [&'static str]) -> Self {
        let _ = names;
        todo!()
    }

    /// Permits any request header.
    #[must_use]
    pub fn allow_any_header(self) -> Self {
        todo!()
    }

    /// Makes these response headers readable by the client.
    #[must_use]
    pub fn expose_headers(self, names: &'static [&'static str]) -> Self {
        let _ = names;
        todo!()
    }

    /// How long a preflight result may be cached.
    #[must_use]
    pub fn max_age(self, age: std::time::Duration) -> Self {
        let _ = age;
        todo!()
    }

    /// Permits credentialed requests.
    #[must_use]
    pub fn allow_credentials(self) -> Self {
        todo!()
    }

    /// Also declares the CORS response headers in the description.
    #[must_use]
    pub fn document_response_headers(self) -> Self {
        todo!()
    }
}

impl<C: Sync + 'static> Interceptor<C> for Cors {
    fn contribution(&self, _route: Route<'_>) -> OperationContribution {
        todo!()
    }

    async fn intercept(
        &self,
        request: http::Request,
        context: &C,
        next: Next<'_, C>,
    ) -> http::Response {
        let _ = (request, context, next);
        todo!()
    }
}
