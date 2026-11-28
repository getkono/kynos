//! Rate limiting.
//!
//! Kynos supplies the description, the 429, the headers and — through
//! [`Quotas`] — a sliding-window algorithm over named quotas.
//! The application supplies *where the counters live*.
//!
//! # How this module is laid out
//!
//! [`decision`] is what a policy reports, [`store`] is where counters live,
//! [`key`] is what a request counts against, [`clock`] is where the time comes
//! from, [`quota`] is the algorithm over the four, [`headers`] is what an
//! allowed exchange says on the wire and [`refusal`] is what a denied one says.
//! The interceptor itself is here.

pub mod clock;
pub mod decision;
pub mod headers;
pub mod key;
pub mod quota;
pub mod refusal;
pub mod store;

mod estimate;

use std::marker::PhantomData;

use crate::middleware::rate_limit::{
    clock::Clock,
    decision::{Decision, QuotaPolicy, RateLimitPolicy, ServiceLimit},
    headers::{RateLimitFields, RateLimitHeaders},
    key::ByHandler,
    quota::Quotas,
    refusal::{RateLimited, RateLimitedFields},
    store::RateLimitStore,
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
        RateLimited::new(retry_after, refused(limits).map_or(0, |limit| limit.quota))
    }
}

/// The limit a denial was refused by: the exhausted one the client waits
/// longest on, which is the one `Retry-After` describes.
///
/// The first consulted where none reports itself exhausted, so a policy that
/// refuses on other grounds still names a ceiling.
fn refused(limits: &[ServiceLimit]) -> Option<&ServiceLimit> {
    limits
        .iter()
        .filter(|limit| limit.remaining == 0)
        // `rev` so that of equal resets the first consulted wins.
        .rev()
        .max_by_key(|limit| limit.reset)
        .or_else(|| limits.first())
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
///         key::AdmittedReads,
///     },
///     router::operation::Route,
/// };
///
/// #[derive(Clone, Debug)]
/// struct PerClient;
///
/// impl RateLimitPolicy<()> for PerClient {
///     // Keyed on no guard's admission, so the router places it anywhere.
///     fn reads_admitted(&self) -> AdmittedReads {
///         AdmittedReads::none()
///     }
///
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
    /// #     RateLimit, decision::{Decision, RateLimitPolicy, ServiceLimit}, key::AdmittedReads,
    /// # }, router::operation::Route};
    /// # #[derive(Clone, Debug)] struct PerClient;
    /// # impl RateLimitPolicy<()> for PerClient {
    /// #     fn reads_admitted(&self) -> AdmittedReads { AdmittedReads::none() }
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
    /// #     RateLimit, decision::{Decision, RateLimitPolicy, ServiceLimit}, key::AdmittedReads,
    /// # }, router::operation::Route};
    /// # #[derive(Clone, Debug)] struct PerClient;
    /// # impl RateLimitPolicy<()> for PerClient {
    /// #     fn reads_admitted(&self) -> AdmittedReads { AdmittedReads::none() }
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

impl<S, Clk, D, T> RateLimit<Quotas<ByHandler, S, Clk>, D, T>
where
    S: RateLimitStore,
    Clk: Clock,
    D: RateLimitSpelling<T>,
    T: ProblemType,
{
    /// Spends one request against `partition`, from a handler.
    ///
    /// For a limit keyed on what only the decoded request says, such as a
    /// username in the body. The refusal is the one this limiter would answer
    /// with as an interceptor — the same problem type, `Retry-After` and
    /// rate-limit fields — so a handler returns it through a transparent
    /// variant of its error, which declares the 429 and its headers on the
    /// operation:
    ///
    /// ```
    /// # #[cfg(feature = "macros")]
    /// # {
    /// use kynos::middleware::rate_limit::{RateLimit, key::ByHandler, quota::Quotas};
    /// #[derive(Debug, thiserror::Error, kynos::ApiError)]
    /// enum ChallengeError {
    ///     #[error("too many challenges for this username")]
    ///     #[problem(transparent, from)]
    ///     Throttled(kynos::middleware::rate_limit::refusal::RateLimited),
    /// }
    ///
    /// async fn challenge<S: kynos::middleware::rate_limit::store::RateLimitStore>(
    ///     limit: &RateLimit<Quotas<ByHandler, S>>,
    ///     username: &str,
    /// ) -> Result<(), ChallengeError> {
    ///     limit.spend(username).await?;
    ///     Ok(())
    /// }
    /// # }
    /// ```
    ///
    /// `examples/rate_limit.rs` keeps one in its context. `Ok` holds the fields an allowed response may carry, for a
    /// [`WithHeaders`](crate::response::headers::WithHeaders).
    ///
    /// The partition is stored under a `handler:` prefix, so a value the
    /// client chose cannot name another key's bucket in a shared store. Every
    /// distinct value is a counter the store keeps for two windows, so a
    /// partition read from the body is as many counters as clients can
    /// invent: validate it first, and bound how fast one client can invent
    /// them with an interceptor limit keyed on its address.
    ///
    /// # Errors
    ///
    /// The refusal, where any quota is spent for `partition`, or the store
    /// cannot answer and [`StoreFailure::Deny`](store::StoreFailure::Deny)
    /// was chosen.
    pub async fn spend(&self, partition: &str) -> Result<D::Headers, D::Denied> {
        let policies = &self.policy.policies;
        match self
            .policy
            .count(Some(&format!("handler:{partition}")))
            .await
        {
            Decision::Allow(allowance) => Ok(D::allow(&allowance.limits, policies)),
            Decision::Deny(denial) => Err(D::deny(denial.retry_after, &denial.limits, policies)),
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

    const READS_BODY: bool = false;

    fn __reads_admitted(&self) -> Vec<crate::__private::GuardType> {
        self.policy.reads_admitted().guards
    }

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<D::Headers>, D::Denied> {
        let () = reads;

        // A policy is keyed by the operation, and a fallback has none, so it is
        // neither counted nor told how much is left.
        let Some(route) = next.route() else {
            return Ok(next.run(request).await.unrouted());
        };

        let policies = self.policy.advertised();
        match self.policy.check(&request, route, context).await {
            Decision::Allow(allowance) => Ok(next
                .run(request)
                .await
                .with_headers(D::allow(&allowance.limits, policies))),
            Decision::Deny(denial) => Err(D::deny(denial.retry_after, &denial.limits, policies)),
        }
    }
}

// Written out because `#[derive]` would bound the phantom `D` and `T`.
marker_traits!(RateLimit<P, D, T> where P { _spelling: _, policy }: Clone, Debug);

#[cfg(test)]
mod tests;
