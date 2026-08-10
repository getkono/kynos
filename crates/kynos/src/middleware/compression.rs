//! Response compression.
//!
//! Out-of-document: content coding is transport, and OpenAPI does not model it.

use crate::{
    http,
    middleware::{Interceptor, Next, contribution::OperationContribution},
    router::operation::Route,
};

/// Compresses responses when the client accepts it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compression {
    _private: (),
}

impl Compression {
    /// Enables every compiled-in algorithm.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }

    /// Skips responses smaller than `bytes`.
    #[must_use]
    pub fn min_size(self, bytes: u64) -> Self {
        let _ = bytes;
        todo!()
    }
}

impl<C: Sync + 'static> Interceptor<C> for Compression {
    fn contribution(&self, _route: Route<'_>) -> OperationContribution {
        OperationContribution::none()
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
