//! What a rate-limit policy reports, and what a response says about it.

use std::{borrow::Cow, time::Duration};

use crate::{http, router::operation::Route};

/// The unit a quota counts in.
///
/// The `qu` parameter of `draft-ietf-httpapi-ratelimit-headers`.
/// `concurrent-requests` is deliberately absent: that is
/// [`Concurrency`](crate::middleware::limits::Concurrency)'s job, and it
/// consumes no rate window.
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
/// Configuration rather than state: what the service *will* enforce, which is
/// the same for every request a limiter covers.
#[derive(Clone, Debug, PartialEq, Eq)]
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
/// different clients, which is why this is a separate type.
#[derive(Clone, Debug, PartialEq, Eq)]
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
pub struct Allowance {
    /// Every limit consulted, in the order they should be reported.
    pub limits: Vec<ServiceLimit>,
}

/// What a policy reports when a request may not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Denial {
    /// How long the client should wait before retrying.
    ///
    /// The policy's to compute, because the policy owns the counters. Kynos
    /// will not invent one: a number derived from a window's *length* rather
    /// than from its remaining time is one the service cannot honour.
    pub retry_after: Duration,
    /// Every limit consulted, including the one that refused.
    pub limits: Vec<ServiceLimit>,
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
        Self::Allow(Allowance {
            limits: vec![limit],
        })
    }

    /// Refuses the request, reporting one limit.
    #[must_use]
    pub fn deny(retry_after: Duration, limit: ServiceLimit) -> Self {
        Self::Deny(Denial {
            retry_after,
            limits: vec![limit],
        })
    }
}

/// Application policy used to identify clients and maintain counters.
///
/// Kynos supplies the description, the 429 and the headers; how a client is
/// identified and where the counters live is the application's, because
/// prescribing a store would mean prescribing a dependency.
/// [`Quotas`](super::quota::Quotas) is the implementation Kynos ships over a
/// store *you* supply.
pub trait RateLimitPolicy<C>: Send + Sync + 'static {
    /// The quota policies this limiter advertises, in report order.
    ///
    /// Borrowed and read once per response: these are configuration, so a
    /// limiter returning them by value would allocate per request for
    /// something that never changes.
    fn advertised(&self) -> &[QuotaPolicy] {
        &[]
    }

    /// Decides whether this request may continue.
    ///
    /// `route` is the `paths` key rather than the request path, so a policy
    /// keying on the operation has bounded cardinality — the same property
    /// [`MatchedPath`](crate::extract::connection::MatchedPath) exists for.
    fn check(
        &self,
        request: &http::Request,
        route: Route<'_>,
        context: &C,
    ) -> impl Future<Output = Decision> + Send;
}
