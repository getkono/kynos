//! Rate limiting.
//!
//! Kynos supplies the description, the 429, the headers and — through
//! [`Quotas`] — a sliding-window algorithm over named quotas. What stays the
//! application's is *where the counters live*, because prescribing a store would
//! mean prescribing a dependency.
//!
//! # How this module is laid out
//!
//! [`decision`] is what a policy reports, [`store`] is where counters live,
//! [`key`] is what a request counts against, [`quota`] is the algorithm over the
//! three, and [`headers`] is what any of it says on the wire. The interceptor
//! itself is here.

pub mod decision;
pub mod headers;
pub mod key;
pub mod quota;
pub mod store;

use std::marker::PhantomData;

pub use decision::{
    Allowance, Decision, Denial, QuotaPolicy, QuotaUnit, RateLimitPolicy, ServiceLimit,
};
pub use headers::{RateLimitFields, RateLimitHeaders, RateLimited, RateLimitedFields};
pub use quota::{Quota, Quotas};
pub use store::{RateLimitStore, StoreFailure};

use crate::{
    extract::params::header::HeaderParams,
    http,
    middleware::{Continued, Interceptor, Next},
    response::ShortCircuit,
};

mod sealed {
    pub trait Sealed {}
}

/// Which spelling of the rate-limit fields a limiter emits.
///
/// Sealed, and there are exactly two. A third would emit a field name nobody
/// reviewed, and the whole reason this is a choice rather than a default is that
/// the names reach generated clients.
pub trait RateLimitSpelling: sealed::Sealed + Send + Sync + 'static {
    /// The group a forwarded response carries.
    type Headers: HeaderParams;
    /// What a refusal answers with.
    type Denied: ShortCircuit;

    /// Builds the group for a request that was allowed.
    fn allow(limits: &[ServiceLimit], policies: &[QuotaPolicy]) -> Self::Headers;

    /// Builds the refusal for a request that was not.
    fn deny(
        retry_after: std::time::Duration,
        limits: &[ServiceLimit],
        policies: &[QuotaPolicy],
    ) -> Self::Denied;
}

/// `X-RateLimit-Limit`, `-Remaining` and `-Reset`.
///
/// The default while `draft-ietf-httpapi-ratelimit-headers` is a draft. See
/// [`RateLimitHeaders`] for why the prefix is deliberate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Legacy;

/// `RateLimit` and `RateLimit-Policy`, per the draft.
///
/// Reached through [`RateLimit::standard_fields`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Structured;

impl sealed::Sealed for Legacy {}
impl sealed::Sealed for Structured {}

impl RateLimitSpelling for Legacy {
    type Headers = RateLimitHeaders;
    type Denied = RateLimited;

    fn allow(limits: &[ServiceLimit], policies: &[QuotaPolicy]) -> Self::Headers {
        let _ = policies;
        RateLimitHeaders::from_limits(limits)
    }

    fn deny(
        retry_after: std::time::Duration,
        limits: &[ServiceLimit],
        policies: &[QuotaPolicy],
    ) -> Self::Denied {
        let _ = policies;
        RateLimited {
            retry_after,
            limit: limits.first().map_or(0, |limit| limit.quota),
        }
    }
}

impl RateLimitSpelling for Structured {
    type Headers = RateLimitFields;
    type Denied = RateLimitedFields;

    fn allow(limits: &[ServiceLimit], policies: &[QuotaPolicy]) -> Self::Headers {
        RateLimitFields {
            limits: limits.to_vec(),
            policies: policies.to_vec(),
        }
    }

    fn deny(
        retry_after: std::time::Duration,
        limits: &[ServiceLimit],
        policies: &[QuotaPolicy],
    ) -> Self::Denied {
        RateLimitedFields {
            retry_after,
            limits: limits.to_vec(),
            policies: policies.to_vec(),
        }
    }
}

/// Limits request rate per client.
///
/// Contributes 429, a `Retry-After` header, and whichever rate-limit fields the
/// spelling names.
///
/// ```no_run
/// use std::time::Duration;
/// use kynos::{
///     http,
///     middleware::rate_limit::{Decision, RateLimit, RateLimitPolicy, ServiceLimit},
///     router::operation::Route,
/// };
///
/// #[derive(Clone, Debug)]
/// struct PerClient;
///
/// impl RateLimitPolicy<()> for PerClient {
///     async fn check(&self, _: &http::Request, _: Route<'_>, _: &()) -> Decision {
///         Decision::allow(ServiceLimit {
///             name: "default".into(),
///             quota: 100,
///             remaining: 99,
///             reset: Duration::from_secs(30),
///         })
///     }
/// }
///
/// let limit = RateLimit::new(PerClient);
/// # let _ = limit;
/// ```
#[derive(Clone, Debug)]
pub struct RateLimit<P, D = Legacy> {
    policy: P,
    _spelling: PhantomData<fn() -> D>,
}

impl<P> RateLimit<P, Legacy> {
    /// Limits requests according to `policy`.
    ///
    /// There is no ceiling argument beside it. The policy reports every quota it
    /// enforced, so the number a response prints and the number a counter
    /// checked are one fact — where a separately configured ceiling is two that
    /// drift.
    #[must_use]
    pub fn new(policy: P) -> Self {
        Self {
            policy,
            _spelling: PhantomData,
        }
    }

    /// Emits `RateLimit` and `RateLimit-Policy` instead of the `X-` triple.
    ///
    /// Changes the type, because it changes what every covered operation
    /// declares and what every generated client reads — the same reason
    /// [`Cors::document_response_headers`](crate::middleware::cors::Cors::document_response_headers)
    /// is a type-state rather than a flag.
    ///
    /// The two are never emitted together. A response carrying both spellings is
    /// two statements of one fact, which is the objection this codebase raises
    /// against a `contribution` method.
    #[must_use]
    pub fn standard_fields(self) -> RateLimit<P, Structured> {
        RateLimit {
            policy: self.policy,
            _spelling: PhantomData,
        }
    }
}

impl<C, P, D> Interceptor<C> for RateLimit<P, D>
where
    C: Sync + 'static,
    P: RateLimitPolicy<C>,
    D: RateLimitSpelling,
{
    type Reads = ();
    type Adds = D::Headers;
    type Short = D::Denied;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<D::Headers>, D::Denied> {
        let () = reads;

        let policies = self.policy.advertised();
        match self.policy.check(&request, next.route(), context).await {
            Decision::Allow(allowance) => Ok(next
                .run(request)
                .await
                .with_headers(D::allow(&allowance.limits, policies))),
            Decision::Deny(denial) => Err(D::deny(denial.retry_after, &denial.limits, policies)),
        }
    }
}

#[cfg(test)]
mod tests;
