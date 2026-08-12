//! Rate limiting.

use std::{future::Future, time::Duration};

use crate::{
    error::problem::Problem,
    http,
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// What [`RateLimit`] answers with when a policy denies a request.
///
/// Carries its own `Retry-After`, which the policy already computed: a
/// [`Decision::Deny`] knows the delay, and the type that reports it is the type
/// that describes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimited {
    /// How long the client should wait before retrying.
    pub retry_after: Duration,
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

        response
    }
}

impl ShortCircuit for RateLimited {
    const STATUSES: &'static [u16] = &[429];
}

impl Responses for RateLimited {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        kynos_openapi::Responses::new().with(
            429,
            kynos_openapi::Response::new("the client has exceeded its request rate").with_header(
                "Retry-After",
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(
                    kynos_openapi::model::schema::types::SchemaType::String,
                ))
                .with_description(
                    "How long to wait before retrying, in seconds or as an HTTP-date",
                ),
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
/// Contributes 429, a `Retry-After` header, and the `RateLimit-*` headers.
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
///         Decision::Allow { remaining: 99 }
///     }
/// }
///
/// let limit = RateLimit::new(100, Duration::from_secs(60), PerClient);
/// # let _ = limit;
/// ```
#[derive(Clone, Debug)]
pub struct RateLimit<P> {
    policy: P,
    requests: u32,
    window: std::time::Duration,
}

impl<P> RateLimit<P> {
    /// Allows `requests` per `window`, consulting `policy` for each request.
    #[must_use]
    pub fn new(requests: u32, window: std::time::Duration, policy: P) -> Self {
        Self {
            policy,
            requests,
            window,
        }
    }
}

impl<C: Sync + 'static, P: RateLimitPolicy<C>> Interceptor<C> for RateLimit<P> {
    type Reads = ();
    type Adds = ();
    type Short = RateLimited;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, RateLimited> {
        let _ = (
            &self.policy,
            self.requests,
            self.window,
            request,
            reads,
            context,
            next,
        );
        todo!()
    }
}
