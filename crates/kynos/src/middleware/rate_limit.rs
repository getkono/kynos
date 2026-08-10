//! Rate limiting.

use std::future::Future;

use crate::http;
use crate::middleware::{Interceptor, Next, contribution::OperationContribution};

/// The result of consulting a rate-limit policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The request may continue.
    Allow {
        /// Requests remaining in the current window.
        remaining: u32,
    },
    /// The request must receive 429 without calling the handler.
    Deny {
        /// How long the client should wait before retrying.
        retry_after: std::time::Duration,
    },
}

/// Application policy used to identify clients and maintain counters.
pub trait RateLimitPolicy<C>: Send + Sync + 'static {
    /// Decides whether this request may continue.
    fn check(&self, request: &http::Request, context: &C) -> impl Future<Output = Decision> + Send;
}

/// Limits request rate per client.
///
/// Contributes 429, a `Retry-After` header, and the `RateLimit-*` headers.
/// Kynos supplies the description and the response; the *policy* — how a
/// client is identified, where counters live — is the application's, since
/// prescribing a store would mean prescribing a dependency.
///
/// ```no_run
/// use std::time::Duration;
/// use kynos::{
///     http,
///     middleware::rate_limit::{Decision, RateLimit, RateLimitPolicy},
/// };
///
/// #[derive(Clone, Debug)]
/// struct PerClient;
///
/// impl RateLimitPolicy<()> for PerClient {
///     async fn check(&self, _: &http::Request, _: &()) -> Decision {
///         Decision::Allow { remaining: 99 }
///     }
/// }
///
/// let limit = RateLimit::new(100, Duration::from_secs(60), PerClient);
/// # let _ = limit;
/// ```
#[derive(Clone, Debug)]
pub struct RateLimit<P> {
    policy: P,
    requests: u32,
    window: std::time::Duration,
}

impl<P> RateLimit<P> {
    /// Allows `requests` per `window`, consulting `policy` for each request.
    #[must_use]
    pub fn new(requests: u32, window: std::time::Duration, policy: P) -> Self {
        Self {
            policy,
            requests,
            window,
        }
    }

    /// This interceptor's contribution.
    #[must_use]
    pub fn contribution(&self) -> OperationContribution {
        todo!()
    }
}

impl<C: Sync + 'static, P: RateLimitPolicy<C>> Interceptor<C> for RateLimit<P> {
    fn contribution(&self) -> OperationContribution {
        Self::contribution(self)
    }

    async fn intercept(
        &self,
        request: http::Request,
        context: &C,
        next: Next<'_, C>,
    ) -> http::Response {
        let _ = (
            &self.policy,
            self.requests,
            self.window,
            request,
            context,
            next,
        );
        todo!()
    }
}
