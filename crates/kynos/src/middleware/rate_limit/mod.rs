//! Rate limiting.
//!
//! Kynos supplies the description, the 429, the headers and — through
//! [`Quotas`](quota::Quotas) — a sliding-window algorithm over named quotas.
//! What stays the application's is *where the counters live*, because
//! prescribing a store would mean prescribing a dependency.
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
    refusal::{RateLimited, RateLimitedFields, RefusalType},
};
use crate::{
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
/// Sealed, and there are exactly two. A third would emit a field name nobody
/// reviewed, and the whole reason this is a choice rather than a default is that
/// the names reach generated clients.
/// `T` is the problem type a refusal carries, threaded through rather than
/// chosen here: which fields a response spells and which type its 429 names are
/// independent decisions, and a service wanting the draft's fields must not
/// lose the URI by taking them.
pub trait RateLimitSpelling<T: RefusalType>: sealed::Sealed + Send + Sync + 'static {
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

impl<T: RefusalType> RateLimitSpelling<T> for Legacy {
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

impl<T: RefusalType> RateLimitSpelling<T> for Structured {
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
pub struct RateLimit<P, D = Legacy, T = ()> {
    policy: P,
    _spelling: PhantomData<fn() -> (D, T)>,
}

impl<P> RateLimit<P, Legacy, ()> {
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
}

impl<P, T> RateLimit<P, Legacy, T> {
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
    ///
    /// A problem type named by [`refusal_type`](RateLimit::refusal_type)
    /// survives the change: the two are independent decisions.
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
    /// Changes the type, for the reason
    /// [`standard_fields`](RateLimit::standard_fields) does: it changes what
    /// every covered operation declares. Stated once, and read by both the
    /// response body and the description — see
    /// [`refusal::RefusalType`] for why that cannot be a value.
    ///
    /// Available only on a limiter that has not named one, so a chain states
    /// the type at most once and a reader never has to find the last call
    /// that won.
    ///
    /// ```no_run
    /// # use std::time::Duration;
    /// # use kynos::{http, middleware::rate_limit::{
    /// #     RateLimit, decision::{Decision, RateLimitPolicy, ServiceLimit},
    /// #     refusal::RefusalType,
    /// # }, router::operation::Route};
    /// # #[derive(Clone, Debug)] struct PerClient;
    /// # impl RateLimitPolicy<()> for PerClient {
    /// #     async fn check(&self, _: &http::Request, _: Route<'_>, _: &()) -> Decision {
    /// #         Decision::allow(ServiceLimit {
    /// #             name: "default".into(), quota: 100, remaining: 99,
    /// #             reset: Duration::from_secs(30),
    /// #         })
    /// #     }
    /// # }
    /// struct Throttled;
    ///
    /// impl RefusalType for Throttled {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/rate-limited");
    /// }
    ///
    /// let limit = RateLimit::new(PerClient).refusal_type::<Throttled>();
    /// # let _ = limit;
    /// ```
    ///
    /// Naming a second one does not compile — the `impl` block is on
    /// `RateLimit<P, D, ()>`, so the method is simply not there once `T` is a
    /// type. The block above is this rule's pass control: the two differ only
    /// in the second call.
    ///
    /// ```compile_fail
    /// # use std::time::Duration;
    /// # use kynos::{http, middleware::rate_limit::{
    /// #     RateLimit, decision::{Decision, RateLimitPolicy, ServiceLimit},
    /// #     refusal::RefusalType,
    /// # }, router::operation::Route};
    /// # #[derive(Clone, Debug)] struct PerClient;
    /// # impl RateLimitPolicy<()> for PerClient {
    /// #     async fn check(&self, _: &http::Request, _: Route<'_>, _: &()) -> Decision {
    /// #         Decision::allow(ServiceLimit {
    /// #             name: "default".into(), quota: 100, remaining: 99,
    /// #             reset: Duration::from_secs(30),
    /// #         })
    /// #     }
    /// # }
    /// struct Throttled;
    /// # impl RefusalType for Throttled {
    /// #     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/throttled");
    /// # }
    /// struct Overdrawn;
    /// # impl RefusalType for Overdrawn {
    /// #     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/overdrawn");
    /// # }
    ///
    /// let limit = RateLimit::new(PerClient)
    ///     .refusal_type::<Throttled>()
    ///     .refusal_type::<Overdrawn>();
    /// # let _ = limit;
    /// ```
    #[must_use]
    pub fn refusal_type<T: RefusalType>(self) -> RateLimit<P, D, T> {
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
    T: RefusalType,
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

// The two derivable implementations, written out. `#[derive]` bounds every
// parameter, and `D` and `T` are names rather than values here: the struct
// holds a `PhantomData<fn() -> (D, T)>` and no instance of either. Derived, a
// limiter naming a problem type would lose `Clone` and `Debug` unless the
// application's marker derived them too -- undoing, one type down, exactly what
// [`refusal`]'s eight hand-written implementations buy.

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
        // One field: the spelling and the problem type are in the type's name,
        // and `_spelling` holds nothing an operator can read.
        formatter
            .debug_struct("RateLimit")
            .field("policy", &self.policy)
            .finish()
    }
}

#[cfg(test)]
mod tests;
