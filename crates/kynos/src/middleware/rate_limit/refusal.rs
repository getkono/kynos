//! What a rate-limited exchange says when a policy refuses.
//!
//! The 429 itself, in both spellings: the problem document it carries, the
//! `Retry-After` beside it, and the description of both. Separate from
//! [`headers`](super::headers), which is what an *allowed* exchange says.

use std::time::Duration;

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    error::problem::{Problem, problem_response},
    extract::params::header::{EncodeHeaders, HeaderParams},
    http,
    middleware::rate_limit::{
        decision::{QuotaPolicy, ServiceLimit},
        headers::{RateLimitFields, RateLimitHeaders, whole_seconds},
    },
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// Describes `Retry-After`, which is a delta-seconds count or an HTTP-date.
fn retry_after_header() -> kynos_openapi::Header {
    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
        .with_description("How long to wait before retrying, in seconds or as an HTTP-date")
}

/// What a limiter answers with when a policy refuses, in the `X-` spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimited {
    /// How long the client should wait before retrying.
    pub retry_after: Duration,
    /// The ceiling that was exceeded.
    pub limit: u64,
}

impl IntoResponse for RateLimited {
    fn into_response(self) -> http::Response {
        let mut response = refusal();
        set_retry_after(&mut response, self.retry_after);

        // The same three a success carries. A denial's reset *is* its retry
        // delay, so reporting it lands no new obligation on the policy.
        write_group(
            &mut response,
            &RateLimitHeaders {
                limit: self.limit,
                remaining: 0,
                reset: self.retry_after,
            },
        );

        response
    }
}

impl ShortCircuit for RateLimited {
    const STATUSES: &'static [u16] = &[429];
}

impl Responses for RateLimited {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let group = RateLimitHeaders::response_headers(registry);
        described_refusal(registry, group)
    }
}

/// The same, in the draft's spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimitedFields {
    /// How long the client should wait before retrying.
    pub retry_after: Duration,
    /// Where the client stands against each policy.
    pub limits: Vec<ServiceLimit>,
    /// What the service enforces.
    pub policies: Vec<QuotaPolicy>,
}

impl IntoResponse for RateLimitedFields {
    fn into_response(self) -> http::Response {
        let mut response = refusal();
        set_retry_after(&mut response, self.retry_after);
        write_group(
            &mut response,
            &RateLimitFields {
                limits: self.limits,
                policies: self.policies,
            },
        );
        response
    }
}

impl ShortCircuit for RateLimitedFields {
    const STATUSES: &'static [u16] = &[429];
}

impl Responses for RateLimitedFields {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let group = RateLimitFields::response_headers(registry);
        described_refusal(registry, group)
    }
}

/// The 429 both spellings share.
fn refusal() -> http::Response {
    Problem::new(http::StatusCode::TOO_MANY_REQUESTS)
        .with_detail("the client has exceeded its request rate")
        .into_response()
}

/// The 429's description, plus whichever header group produced it.
fn described_refusal(
    registry: &mut Registry,
    group: kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>>,
) -> kynos_openapi::Responses {
    kynos_openapi::Responses::new().with(
        429,
        group.into_iter().fold(
            problem_response(registry, "the client has exceeded its request rate")
                .with_header("Retry-After", retry_after_header()),
            |response, (name, header)| match header {
                kynos_openapi::RefOr::Item(header) => response.with_header(name, header),
                kynos_openapi::RefOr::Ref(_) => response,
            },
        ),
    )
}

fn set_retry_after(response: &mut http::Response, retry_after: Duration) {
    if let Ok(value) = http::HeaderValue::from_str(&whole_seconds(retry_after).to_string()) {
        response
            .headers_mut()
            .insert(http::header::RETRY_AFTER, value);
    }
}

/// Writes a group onto a short-circuit response.
///
/// Through the one writer, so a short circuit and a forwarded response spell a
/// group the same way.
fn write_group<G: EncodeHeaders>(response: &mut http::Response, group: &G) {
    crate::extract::params::header::write(response.headers_mut(), group);
}
