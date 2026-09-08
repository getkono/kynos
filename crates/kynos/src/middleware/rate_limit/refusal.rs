//! What a rate-limited exchange says when a policy refuses.
//!
//! The 429 itself, in both spellings: the problem document it carries, the
//! `Retry-After` beside it, and the description of both. Separate from
//! [`headers`](super::headers), which is what an *allowed* exchange says.

use std::{fmt, marker::PhantomData, time::Duration};

use kynos_openapi::model::{body::mime_names::APPLICATION_PROBLEM_JSON, schema::types::SchemaType};

use crate::{
    error::problem::{Problem, ProblemType, problem_response},
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
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`.
pub struct RateLimited<T = ()> {
    /// How long the client should wait before retrying.
    pub retry_after: Duration,
    /// The ceiling that was exceeded.
    pub limit: u64,
    /// Carries `T` without storing one. `fn() -> T` rather than `T`, so a
    /// refusal is `Send` and `Sync` whatever the marker is.
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
        let mut response = refusal_problem::<T>().into_response();
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
        let mut response = refusal_problem::<T>().into_response();
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

impl<T: ProblemType> ShortCircuit for RateLimitedFields<T> {
    const STATUSES: &'static [u16] = &[429];
}

impl<T: ProblemType> Responses for RateLimitedFields<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let group = RateLimitFields::response_headers(registry);
        described_refusal::<T>(registry, group)
    }
}

/// The problem both halves of a refusal read.
///
/// One function, because the document is a claim about what the wire carries
/// and two constructions of "the same" problem are how the two came to
/// disagree. The title stays the status code's reason phrase whether or not a
/// type was named: it summarises the problem type, and "Too Many Requests"
/// summarises every rate-limit refusal there is.
fn refusal_problem<T: ProblemType>() -> Problem {
    let mut problem = Problem::new(http::StatusCode::TOO_MANY_REQUESTS)
        .with_detail("the client has exceeded its request rate");

    if let Some(uri) = T::TYPE_URI {
        problem.type_uri = uri.into();
    }

    problem
}

/// The 429's description, plus whichever header group produced it.
fn described_refusal<T: ProblemType>(
    registry: &mut Registry,
    group: kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>>,
) -> kynos_openapi::Responses {
    let mut response = group.into_iter().fold(
        problem_response(registry, "the client has exceeded its request rate")
            .with_header("Retry-After", retry_after_header()),
        |response, (name, header)| match header {
            kynos_openapi::RefOr::Item(header) => response.with_header(name, header),
            kynos_openapi::RefOr::Ref(_) => response,
        },
    );

    // A named type is *shown* rather than stated, because the `Problem` schema
    // is shared by every error Kynos describes and narrowing `type` to one URI
    // there would narrow it for all of them. An example is what the document
    // can carry today; a `const`-narrowed member is the mechanism that will
    // replace it.
    //
    // Two members and no more. An example is a promise about the wire, and the
    // only members this code fixes are the URI and the status -- `title` and
    // `detail` are English prose an interceptor is free to localize, so
    // showing them would publish a claim no response is held to. These two are
    // exactly what a `const`-narrowed `type` will state instead.
    //
    // Written through the media type `problem_response` already installed,
    // which keeps that function the one writer of this content.
    if let Some(uri) = T::TYPE_URI {
        if let Some(media_type) = response.content.get_mut(APPLICATION_PROBLEM_JSON) {
            let described = std::mem::take(media_type);
            *media_type = described.with_example(serde_json::json!({
                "type": uri,
                "status": 429,
            }));
        }
    }

    kynos_openapi::Responses::new().with(429, response)
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

// The four derivable implementations, written out: `#[derive]` would bound each
// on `T`, and the marker is a name rather than a value -- it is never cloned,
// printed or compared, and requiring it to be would make naming a problem type
// cost four derives on the application's own marker.

impl<T> Clone for RateLimited<T> {
    fn clone(&self) -> Self {
        Self::new(self.retry_after, self.limit)
    }
}

impl<T> fmt::Debug for RateLimited<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Destructured rather than read member by member: `new` and `clone`
        // stop compiling when a field is added, and this makes the two that
        // would otherwise ignore it stop too.
        let Self {
            retry_after,
            limit,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("RateLimited")
            .field("retry_after", retry_after)
            .field("limit", limit)
            .finish()
    }
}

impl<T> PartialEq for RateLimited<T> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            retry_after,
            limit,
            problem_type: _,
        } = self;

        *retry_after == other.retry_after && *limit == other.limit
    }
}

impl<T> Eq for RateLimited<T> {}

impl<T> Clone for RateLimitedFields<T> {
    fn clone(&self) -> Self {
        Self::new(self.retry_after, self.limits.clone(), self.policies.clone())
    }
}

impl<T> fmt::Debug for RateLimitedFields<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            retry_after,
            limits,
            policies,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("RateLimitedFields")
            .field("retry_after", retry_after)
            .field("limits", limits)
            .field("policies", policies)
            .finish()
    }
}

impl<T> PartialEq for RateLimitedFields<T> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            retry_after,
            limits,
            policies,
            problem_type: _,
        } = self;

        *retry_after == other.retry_after && *limits == other.limits && *policies == other.policies
    }
}

impl<T> Eq for RateLimitedFields<T> {}
