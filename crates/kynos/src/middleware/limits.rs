//! Limits, and the responses they make possible.

use crate::http;
use crate::middleware::{Interceptor, Next, contribution::OperationContribution};

/// Caps the size of a request body.
///
/// Contributes 413 to every covered operation — which is the point.
/// Configuring a limit and documenting that the limit exists are the same
/// action, so an API cannot quietly reject payloads it claims to accept.
#[derive(Clone, Copy, Debug)]
pub struct BodySize {
    /// The maximum body size, in bytes.
    pub limit: u64,
}

impl BodySize {
    /// Caps bodies at `bytes`.
    #[must_use]
    pub fn new(bytes: u64) -> Self {
        Self { limit: bytes }
    }

    /// This interceptor's contribution.
    #[must_use]
    pub fn contribution(&self) -> OperationContribution {
        todo!()
    }
}

impl<C: Sync + 'static> Interceptor<C> for BodySize {
    fn contribution(&self) -> OperationContribution {
        Self::contribution(self)
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

/// Caps how long a handler may run.
///
/// Contributes 504.
#[derive(Clone, Copy, Debug)]
pub struct Timeout {
    /// The maximum handler duration.
    pub limit: std::time::Duration,
}

impl Timeout {
    /// Limits handlers to `limit`.
    pub fn new(limit: std::time::Duration) -> Self {
        Self { limit }
    }

    /// This interceptor's contribution.
    pub fn contribution(&self) -> OperationContribution {
        todo!()
    }
}

impl<C: Sync + 'static> Interceptor<C> for Timeout {
    fn contribution(&self) -> OperationContribution {
        Self::contribution(self)
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

/// Caps concurrent in-flight requests.
///
/// Contributes 503 and a `Retry-After` response header.
#[derive(Clone, Copy, Debug)]
pub struct Concurrency {
    /// The maximum number of requests in flight at once.
    pub limit: usize,
}

impl Concurrency {
    /// Limits in-flight requests to `limit`.
    pub fn new(limit: usize) -> Self {
        Self { limit }
    }

    /// This interceptor's contribution.
    pub fn contribution(&self) -> OperationContribution {
        todo!()
    }
}

impl<C: Sync + 'static> Interceptor<C> for Concurrency {
    fn contribution(&self) -> OperationContribution {
        Self::contribution(self)
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
