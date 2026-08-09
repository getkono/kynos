//! Correlation identifiers.

use crate::{
    http,
    middleware::{Interceptor, Next, contribution::OperationContribution},
};

/// Assigns each request an identifier and echoes it back.
///
/// This is an interceptor because it adds a response header. Its
/// contribution keeps that wire-visible behavior in every covered
/// operation's description.
#[derive(Clone, Debug, Default)]
pub struct RequestId {
    _private: (),
}

impl RequestId {
    /// Uses `X-Request-Id`, generating one when the client sends none.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }

    /// Uses a different header name.
    #[must_use]
    pub fn header(self, name: &'static str) -> Self {
        let _ = name;
        todo!()
    }
}

impl<C: Sync + 'static> Interceptor<C> for RequestId {
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
