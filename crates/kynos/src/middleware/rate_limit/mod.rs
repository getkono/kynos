//! Rate limiting.
//!
//! Kynos supplies the description, the 429, the headers and — through
//! [`Quotas`](quota::Quotas) — a sliding-window algorithm over named quotas.
//! The application supplies *where the counters live*.
//!
//! # How this module is laid out
//!
//! [`decision`] is what a policy reports, [`store`] is where counters live,
//! [`key`] is what a request counts against, [`quota`] is the algorithm over the
//! three, [`headers`] is what an allowed exchange says on the wire and
//! [`refusal`] is what a denied one says. The interceptor itself is here.

pub mod decision;
pub mod headers;
pub mod key;
pub mod quota;
pub mod refusal;
pub mod store;

use std::{fmt, marker::PhantomData};

use crate::middleware::rate_limit::{
    decision::{Decision, QuotaPolicy, RateLimitPolicy, ServiceLimit},
    headers::{RateLimitFields, RateLimitHeaders},
    refusal::{RateLimited, RateLimitedFields},
};
use crate::{
    error::problem::ProblemType,
    extract::params::header::EncodeHeaders,
    http,
    middleware::{Continued, Interceptor, Next},
    response::ShortCircuit,
};

mod sealed {
    pub trait Sealed {}
}

/// Which spelling of the rate-limit fields a limiter emits.
///
/// Sealed, and there are exactly two, since the names reach generated clients.
/// `T` is the problem type a refusal carries, independent of the spelling.
pub trait RateLimitSpelling<T: ProblemType>: sealed::Sealed + Send + Sync + 'static {
    /// The group a forwarded response carries.
    type Headers: EncodeHeaders;
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

impl<T: ProblemType> RateLimitSpelling<T> for Legacy {
    type Headers = RateLimitHeaders;
    type Denied = RateLimited<T>;

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
        RateLimited::new(retry_after, limits.first().map_or(0, |limit| limit.quota))
    }
}

impl<T: ProblemType> RateLimitSpelling<T> for Structured {
    type Headers = RateLimitFields;
    type Denied = RateLimitedFields<T>;

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
        RateLimitedFields::new(retry_after, limits.to_vec(), policies.to_vec())
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
///     middleware::rate_limit::{
///         RateLimit,
///         decision::{Decision, RateLimitPolicy, ServiceLimit},
///     },
///     router::operation::Route,
/// };
///
/// #[derive(Clone, Debug)]
/// struct PerClient;
///
/// impl RateLimitPolicy<()> for PerClient {
///     async fn check(&self, _: &http::Request, _: Route<'_>, _: &()) -> Decision {
///         Decision::allow(ServiceLimit::new("default", 100, 99, Duration::from_secs(30)))
///     }
/// }
///
/// let limit = RateLimit::new(PerClient);
/// # let _ = limit;
/// ```
pub struct RateLimit<P, D = Legacy, T = ()> {
    policy: P,
    _spelling: PhantomData<fn() -> (D, T)>,
}

impl<P> RateLimit<P, Legacy, ()> {
    /// Limits requests according to `policy`.
    ///
    /// The policy reports every quota it enforced, so there is no separate
    /// ceiling to drift from what a response prints.
    #[must_use]
    pub fn new(policy: P) -> Self {
        Self {
            policy,
            _spelling: PhantomData,
        }
    }
}

impl<P, T> RateLimit<P, Legacy, T> {
    /// Emits `RateLimit` and `RateLimit-Policy` instead of the `X-` triple.
    ///
    /// Changes the type, because it changes what every covered operation
    /// declares and what every generated client reads. The two spellings are
    /// never emitted together.
    ///
    /// A problem type named by [`problem_type`](RateLimit::problem_type)
    /// survives the change.
    #[must_use]
    pub fn standard_fields(self) -> RateLimit<P, Structured, T> {
        RateLimit {
            policy: self.policy,
            _spelling: PhantomData,
        }
    }
}

impl<P, D> RateLimit<P, D, ()> {
    /// Names the RFC 9457 problem type this limiter's 429 carries.
    ///
    /// Changes the type, since it changes what every covered operation
    /// declares; see [`ProblemType`]. Available only on a limiter that has not
    /// named one, so a chain states the type at most once.
    ///
    /// ```no_run
    /// # use std::time::Duration;
    /// # use kynos::{error::problem::ProblemType, http, middleware::rate_limit::{
    /// #     RateLimit, decision::{Decision, RateLimitPolicy, ServiceLimit},
    /// # }, router::operation::Route};
    /// # #[derive(Clone, Debug)] struct PerClient;
    /// # impl RateLimitPolicy<()> for PerClient {
    /// #     async fn check(&self, _: &http::Request, _: Route<'_>, _: &()) -> Decision {
    /// #         Decision::allow(ServiceLimit::new("default", 100, 99, Duration::from_secs(30)))
    /// #     }
    /// # }
    /// struct Throttled;
    ///
    /// impl ProblemType for Throttled {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/rate-limited");
    /// }
    ///
    /// let limit = RateLimit::new(PerClient).problem_type::<Throttled>();
    /// # let _ = limit;
    /// ```
    ///
    /// Naming a second one does not compile:
    ///
    /// ```compile_fail
    /// # use std::time::Duration;
    /// # use kynos::{error::problem::ProblemType, http, middleware::rate_limit::{
    /// #     RateLimit, decision::{Decision, RateLimitPolicy, ServiceLimit},
    /// # }, router::operation::Route};
    /// # #[derive(Clone, Debug)] struct PerClient;
    /// # impl RateLimitPolicy<()> for PerClient {
    /// #     async fn check(&self, _: &http::Request, _: Route<'_>, _: &()) -> Decision {
    /// #         Decision::allow(ServiceLimit::new("default", 100, 99, Duration::from_secs(30)))
    /// #     }
    /// # }
    /// struct Throttled;
    /// # impl ProblemType for Throttled {
    /// #     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/throttled");
    /// # }
    /// struct Overdrawn;
    /// # impl ProblemType for Overdrawn {
    /// #     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/overdrawn");
    /// # }
    ///
    /// let limit = RateLimit::new(PerClient)
    ///     .problem_type::<Throttled>()
    ///     .problem_type::<Overdrawn>();
    /// # let _ = limit;
    /// ```
    #[must_use]
    pub fn problem_type<T: ProblemType>(self) -> RateLimit<P, D, T> {
        RateLimit {
            policy: self.policy,
            _spelling: PhantomData,
        }
    }
}

impl<C, P, D, T> Interceptor<C> for RateLimit<P, D, T>
where
    C: Sync + 'static,
    P: RateLimitPolicy<C>,
    D: RateLimitSpelling<T>,
    T: ProblemType,
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

// Written out because `#[derive]` would bound the phantom `D` and `T`.

impl<P: Clone, D, T> Clone for RateLimit<P, D, T> {
    fn clone(&self) -> Self {
        Self {
            policy: self.policy.clone(),
            _spelling: PhantomData,
        }
    }
}

impl<P: fmt::Debug, D, T> fmt::Debug for RateLimit<P, D, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Destructured, so a new field is a compile error here.
        let Self {
            policy,
            _spelling: _,
        } = self;

        formatter
            .debug_struct("RateLimit")
            .field("policy", policy)
            .finish()
    }
}

#[cfg(test)]
mod tests;
