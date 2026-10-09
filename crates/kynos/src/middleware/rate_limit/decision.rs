//! What a rate-limit policy reports, and what a response says about it.

use std::{borrow::Cow, time::Duration};

use crate::{http, router::operation::Route};

/// The unit a quota counts in.
///
/// The `qu` parameter of `draft-ietf-httpapi-ratelimit-headers`.
/// `concurrent-requests` is absent: that is
/// [`Concurrency`](crate::middleware::limits::concurrency::Concurrency)'s job.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum QuotaUnit {
    /// Requests. The default, and what a rate limit usually means.
    #[default]
    Requests,
    /// Bytes of request content.
    ContentBytes,
}

impl QuotaUnit {
    /// The token the draft spells this with.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requests => "requests",
            Self::ContentBytes => "content-bytes",
        }
    }
}

/// One quota policy a response advertises.
///
/// Configuration rather than state: the same for every request a limiter
/// covers.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct QuotaPolicy {
    /// The name this policy is reported under.
    pub name: Cow<'static, str>,
    /// How much the policy permits per window.
    pub quota: u64,
    /// The window it permits that much in.
    ///
    /// `None` for a policy with no window — a total allowance rather than a
    /// rate.
    pub window: Option<Duration>,
    /// What the quota counts.
    pub unit: QuotaUnit,
}

/// One live service limit, as it stands for *this* request.
///
/// State rather than configuration: the same policy reports different values to
/// different clients.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceLimit {
    /// The policy this reports against.
    pub name: Cow<'static, str>,
    /// The ceiling the policy permits.
    pub quota: u64,
    /// How much of it is left.
    pub remaining: u64,
    /// How long until the quota is replenished.
    pub reset: Duration,
}

/// What a policy reports when a request may continue.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Allowance {
    /// Every limit consulted, in the order they should be reported.
    pub limits: Vec<ServiceLimit>,
}

/// What a policy reports when a request may not.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Denial {
    /// How long the client should wait before retrying.
    ///
    /// The policy's to compute, because the policy owns the counters.
    pub retry_after: Duration,
    /// Every limit consulted, including the one that refused.
    pub limits: Vec<ServiceLimit>,
}

impl QuotaPolicy {
    /// A policy permitting `quota` per `window`, counted in `unit`.
    ///
    /// `window` is `None` for a total allowance rather than a rate.
    #[must_use]
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        quota: u64,
        window: Option<Duration>,
        unit: QuotaUnit,
    ) -> Self {
        Self {
            name: name.into(),
            quota,
            window,
            unit,
        }
    }
}

impl ServiceLimit {
    /// A limit of `quota`, with `remaining` left until `reset` elapses.
    #[must_use]
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        quota: u64,
        remaining: u64,
        reset: Duration,
    ) -> Self {
        Self {
            name: name.into(),
            quota,
            remaining,
            reset,
        }
    }
}

impl Allowance {
    /// An allowance reporting `limits`, in report order.
    #[must_use]
    pub fn new(limits: Vec<ServiceLimit>) -> Self {
        Self { limits }
    }
}

impl Denial {
    /// A refusal asking the client to wait `retry_after`, reporting `limits`.
    #[must_use]
    pub fn new(retry_after: Duration, limits: Vec<ServiceLimit>) -> Self {
        Self {
            retry_after,
            limits,
        }
    }
}

/// The result of consulting a rate-limit policy.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decision {
    /// The request may continue.
    Allow(Allowance),
    /// The request must receive 429 without reaching the handler.
    Deny(Denial),
}

impl Decision {
    /// Allows the request, reporting one limit.
    #[must_use]
    pub fn allow(limit: ServiceLimit) -> Self {
        Self::Allow(Allowance::new(vec![limit]))
    }

    /// Refuses the request, reporting one limit.
    #[must_use]
    pub fn deny(retry_after: Duration, limit: ServiceLimit) -> Self {
        Self::Deny(Denial::new(retry_after, vec![limit]))
    }
}

/// Application policy used to identify clients and maintain counters.
///
/// Kynos supplies the description, the 429 and the headers; how a client is
/// identified and where the counters live is the application's.
/// [`Quotas`](super::quota::Quotas) is the implementation Kynos ships over a
/// store *you* supply.
pub trait RateLimitPolicy<C>: Send + Sync + 'static {
    /// The quota policies this limiter advertises, in report order.
    ///
    /// Borrowed, since they are configuration read once per response.
    fn advertised(&self) -> &[QuotaPolicy] {
        &[]
    }

    /// Decides whether this request may continue.
    ///
    /// `route` is the `paths` key rather than the request path, so a policy
    /// keying on the operation has bounded cardinality.
    fn check(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> impl Future<Output = Decision> + Send;
}
