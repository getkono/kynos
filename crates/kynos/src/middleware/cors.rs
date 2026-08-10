//! Cross-origin resource sharing.
//!
//! Out-of-document: a preflight `OPTIONS` is a browser protocol detail, not an
//! operation of the API, so it contributes nothing. Use
//! [`Cors::document_response_headers`] when the CORS response headers are part
//! of what you want clients to know about.

use crate::{
    http,
    middleware::{Interceptor, Next, contribution::OperationContribution},
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
    fn contribution(&self) -> OperationContribution {
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
