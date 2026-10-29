//! What a rate-limited exchange says when a policy refuses.
//!
//! The 429 itself, in both spellings: the problem document it carries, the
//! `Retry-After` beside it, and the description of both. Separate from
//! [`headers`](super::headers), which is what an *allowed* exchange says.

use std::{marker::PhantomData, time::Duration};

use crate::{
    error::problem::{ProblemType, refusal_problem, refusal_response},
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{self, retry_after_header, set_retry_after},
    middleware::rate_limit::{
        decision::{QuotaPolicy, ServiceLimit},
        headers::{RateLimitFields, RateLimitHeaders},
    },
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// What a limiter answers with when a policy refuses, in the `X-` spelling.
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`.
pub struct RateLimited<T = ()> {
    /// How long the client should wait before retrying.
    pub retry_after: Duration,
    /// The ceiling that was exceeded.
    pub limit: u64,
    /// Carries `T`; `fn() -> T` keeps a refusal `Send` and `Sync`.
    problem_type: PhantomData<fn() -> T>,
}

impl<T> RateLimited<T> {
    /// A refusal reporting `limit` as the ceiling that was exceeded.
    #[must_use]
    pub fn new(retry_after: Duration, limit: u64) -> Self {
        Self {
            retry_after,
            limit,
            problem_type: PhantomData,
        }
    }
}

impl<T: ProblemType> IntoResponse for RateLimited<T> {
    fn into_response(self) -> http::Response {
        // A denial's reset is its retry delay.
        refusal::<T, _>(
            self.retry_after,
            &RateLimitHeaders {
                limit: self.limit,
                remaining: 0,
                reset: self.retry_after,
            },
        )
    }
}

impl<T: ProblemType> ShortCircuit for RateLimited<T> {
    const STATUSES: &'static [u16] = &[429];
}

impl<T: ProblemType> Responses for RateLimited<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let group = RateLimitHeaders::response_headers(registry);
        described_refusal::<T>(registry, group)
    }
}

/// The same, in the draft's spelling.
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`.
pub struct RateLimitedFields<T = ()> {
    /// How long the client should wait before retrying.
    pub retry_after: Duration,
    /// Where the client stands against each policy.
    pub limits: Vec<ServiceLimit>,
    /// What the service enforces.
    pub policies: Vec<QuotaPolicy>,
    /// Carries `T` without storing one, as in [`RateLimited`].
    problem_type: PhantomData<fn() -> T>,
}

impl<T> RateLimitedFields<T> {
    /// A refusal reporting every limit consulted and every quota advertised.
    #[must_use]
    pub fn new(
        retry_after: Duration,
        limits: Vec<ServiceLimit>,
        policies: Vec<QuotaPolicy>,
    ) -> Self {
        Self {
            retry_after,
            limits,
            policies,
            problem_type: PhantomData,
        }
    }
}

impl<T: ProblemType> IntoResponse for RateLimitedFields<T> {
    fn into_response(self) -> http::Response {
        refusal::<T, _>(
            self.retry_after,
            &RateLimitFields {
                limits: self.limits,
                policies: self.policies,
            },
        )
    }
}

impl<T: ProblemType> ShortCircuit for RateLimitedFields<T> {
    const STATUSES: &'static [u16] = &[429];
}

impl<T: ProblemType> Responses for RateLimitedFields<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let group = RateLimitFields::response_headers(registry);
        described_refusal::<T>(registry, group)
    }
}

/// The 429's description, plus whichever header group produced it.
///
/// [`refusal_response`] makes the named type a `const` in the declared schema.
fn described_refusal<T: ProblemType>(
    registry: &mut Registry,
    group: kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>>,
) -> kynos_openapi::Responses {
    let response = group.into_iter().fold(
        refusal_response::<T>(registry, 429, "the client has exceeded its request rate")
            .with_header("Retry-After", retry_after_header()),
        |response, (name, header)| match header {
            kynos_openapi::RefOr::Item(header) => response.with_header(name, header),
            kynos_openapi::RefOr::Ref(_) => response,
        },
    );

    kynos_openapi::Responses::new().with(429, response)
}

/// The 429 in either spelling: the problem, `Retry-After`, and `group` written
/// through the same writer a forwarded response uses.
fn refusal<T: ProblemType, G: EncodeHeaders>(retry_after: Duration, group: &G) -> http::Response {
    let mut response = refusal_problem::<T>(http::StatusCode::TOO_MANY_REQUESTS)
        .with_detail("the client has exceeded its request rate")
        .into_response();
    set_retry_after(&mut response, retry_after);
    crate::extract::params::header::write_or_refuse(&mut response, group);

    response
}

// Written out because `#[derive]` would bound the phantom marker `T`.
marker_traits!(RateLimited<T> { problem_type: _, retry_after, limit }: Clone, Debug, PartialEq);
marker_traits!(
    RateLimitedFields<T> { problem_type: _, retry_after, limits, policies }: Clone, Debug, PartialEq
);
