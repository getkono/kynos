//! Rate limiting.

use std::{future::Future, time::Duration};

use crate::{
    error::problem::Problem,
    extract::params::header::HeaderParams,
    http,
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// The rate-limit headers a response carries, in the spelling Kynos emits.
///
/// # Why the `X-` prefix
///
/// The unprefixed `RateLimit-Limit` / `-Remaining` / `-Reset` triple belongs to
/// `draft-ietf-httpapi-ratelimit-headers`, which has already *replaced* it with
/// a single structured `RateLimit` field plus `RateLimit-Policy`. These names
/// are [`DESCRIBED`](HeaderParams::DESCRIBED), so they reach generated clients —
/// which makes squatting three names a working group is actively revising
/// expensive rather than cosmetic. The `X-` prefix is unambiguously the
/// application's.
///
/// RFC 6648 deprecates `X-` prefixes for new headers, and that is the
/// acknowledged cost of the choice rather than an oversight.
///
/// When the draft settles, adding the standard spelling is additive: a second
/// `HeaderParams` group and a type-state transition on [`RateLimit`], shaped
/// exactly like
/// [`Cors::document_response_headers`](crate::middleware::cors::Cors::document_response_headers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimitHeaders {
    /// The ceiling the limit was configured with.
    pub limit: u32,
    /// Requests remaining in the current window.
    pub remaining: u32,
    /// How long until the window resets.
    pub reset: Duration,
}

impl HeaderParams for RateLimitHeaders {
    const NAMES: &'static [&'static str] = &[
        "x-ratelimit-limit",
        "x-ratelimit-remaining",
        "x-ratelimit-reset",
    ];

    fn encode(&self) -> Vec<(http::HeaderName, http::HeaderValue)> {
        [
            ("x-ratelimit-limit", self.limit.to_string()),
            ("x-ratelimit-remaining", self.remaining.to_string()),
            ("x-ratelimit-reset", self.reset.as_secs().to_string()),
        ]
        .into_iter()
        .filter_map(|(name, value)| {
            Some((
                http::HeaderName::from_static(name),
                http::HeaderValue::from_str(&value).ok()?,
            ))
        })
        .collect()
    }

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        // Hand-written rather than derived: each of the three is a count, and a
        // count is an integer — where a derive over a `Duration` field would
        // give a string.
        let count = |description: &str| {
            kynos_openapi::RefOr::Item(
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(
                    kynos_openapi::model::schema::types::SchemaType::Integer,
                ))
                .with_description(description),
            )
        };

        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "X-RateLimit-Limit".to_owned(),
            count("Requests permitted per window"),
        );
        headers.insert(
            "X-RateLimit-Remaining".to_owned(),
            count("Requests remaining in the current window"),
        );
        headers.insert(
            "X-RateLimit-Reset".to_owned(),
            count("Seconds until the current window resets"),
        );
        headers
    }
}

/// What [`RateLimit`] answers with when a policy denies a request.
///
/// Carries its own `Retry-After`, which the policy already computed: a
/// [`Decision::Deny`] knows the delay, and the type that reports it is the type
/// that describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimited {
    /// How long the client should wait before retrying.
    pub retry_after: Duration,
    /// The ceiling that was exceeded.
    pub limit: u32,
}

impl IntoResponse for RateLimited {
    fn into_response(self) -> http::Response {
        let mut response = Problem::new(http::StatusCode::TOO_MANY_REQUESTS)
            .with_detail("the client has exceeded its request rate")
            .into_response();

        if let Ok(value) = http::HeaderValue::from_str(&self.retry_after.as_secs().to_string()) {
            response
                .headers_mut()
                .insert(http::header::RETRY_AFTER, value);
        }

        // The same three a success carries. A denial's reset *is* its retry
        // delay, so reporting it lands no new obligation on the policy.
        for (name, value) in (RateLimitHeaders {
            limit: self.limit,
            remaining: 0,
            reset: self.retry_after,
        })
        .encode()
        {
            response.headers_mut().insert(name, value);
        }

        response
    }
}

impl ShortCircuit for RateLimited {
    const STATUSES: &'static [u16] = &[429];
}

impl Responses for RateLimited {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            429,
            RateLimitHeaders::response_headers(registry)
                .into_iter()
                .fold(
                    kynos_openapi::Response::new("the client has exceeded its request rate")
                        .with_header(
                            "Retry-After",
                            kynos_openapi::Header::new(kynos_openapi::Schema::of_type(
                                kynos_openapi::model::schema::types::SchemaType::String,
                            ))
                            .with_description(
                                "How long to wait before retrying, in seconds or as an HTTP-date",
                            ),
                        ),
                    |response, (name, header)| match header {
                        kynos_openapi::RefOr::Item(header) => response.with_header(name, header),
                        kynos_openapi::RefOr::Ref(_) => response,
                    },
                ),
        )
    }
}

/// The result of consulting a rate-limit policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decision {
    /// The request may continue.
    Allow {
        /// Requests remaining in the current window.
        remaining: u32,
        /// How long until the current window resets.
        ///
        /// The policy's to report, because the window's edge is where the
        /// counters are. Computing it from a configured length here would give
        /// the window's *duration* rather than the time to its end — a number
        /// the service cannot honour, which is the same objection
        /// [`limits`](crate::middleware::limits) raises against inventing a
        /// `Retry-After`.
        reset: std::time::Duration,
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
/// Contributes 429, a `Retry-After` header, and the
/// [`X-RateLimit-*`](RateLimitHeaders) headers.
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
///         Decision::Allow {
///             remaining: 99,
///             reset: Duration::from_secs(30),
///         }
///     }
/// }
///
/// let limit = RateLimit::new(100, PerClient);
/// # let _ = limit;
/// ```
#[derive(Clone, Debug)]
pub struct RateLimit<P> {
    policy: P,
    /// The ceiling this reports, which is the one thing about the configured
    /// rate a response can state as a fact.
    ///
    /// There is deliberately no `window` beside it. A window's *length* is not
    /// the time until it resets, and reporting the first as the second would be
    /// a number the service cannot honour; the policy owns the counters, so the
    /// policy is what knows the edge. See [`Decision::Allow`].
    requests: u32,
}

impl<P> RateLimit<P> {
    /// Allows `requests` per window, consulting `policy` for each request.
    ///
    /// The window itself is the policy's: it maintains the counters, so it is
    /// the only thing that can say when one resets.
    #[must_use]
    pub fn new(requests: u32, policy: P) -> Self {
        Self { policy, requests }
    }
}

impl<C: Sync + 'static, P: RateLimitPolicy<C>> Interceptor<C> for RateLimit<P> {
    type Reads = ();
    type Adds = RateLimitHeaders;
    type Short = RateLimited;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<RateLimitHeaders>, RateLimited> {
        let () = reads;

        // The policy owns the counters, so consulting it is the whole of the
        // decision: a denial already carries the delay it computed, which is
        // what the 429 reports and what its description promises.
        match self.policy.check(&request, context).await {
            Decision::Allow { remaining, reset } => {
                Ok(next.run(request).await.with_headers(RateLimitHeaders {
                    limit: self.requests,
                    remaining,
                    reset,
                }))
            }
            Decision::Deny { retry_after } => Err(RateLimited {
                retry_after,
                limit: self.requests,
            }),
        }
    }
}
