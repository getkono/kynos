//! Limits, and the responses they make possible.
//!
//! Each limit here owns a type for the response it answers with. That is what
//! keeps the declaration and the behaviour one fact rather than two: the status
//! a limit can produce is the status its response type describes, and a header
//! that rides that status — `Retry-After` on a 503 — is described by the same
//! type that sets it, rather than by a separate entry keyed on the status.

use std::time::Duration;

use bytes::{Bytes, BytesMut};
use http_body_util::BodyExt;
use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    error::problem::Problem,
    http,
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// Describes `Retry-After`, which is a delta-seconds count or an HTTP-date.
///
/// A string, because the field is one or the other and a schema claiming it is
/// always an integer would be wrong half the time.
fn retry_after_header() -> kynos_openapi::Header {
    kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
        .with_description("How long to wait before retrying, in seconds or as an HTTP-date")
}

/// Sets `Retry-After` on `response` when there is a delay to advertise.
fn set_retry_after(response: &mut http::Response, retry_after: Option<Duration>) {
    // Deliberately not a let-chain: those are stable well above the declared
    // MSRV, and this is not worth raising the floor for.
    let Some(delay) = retry_after else { return };

    if let Ok(value) = http::HeaderValue::from_str(&delay.as_secs().to_string()) {
        response
            .headers_mut()
            .insert(http::header::RETRY_AFTER, value);
    }
}

/// What [`BodySize`] answers with when a body is too large.
///
/// Carries the limit it enforced, so the response can say what was exceeded
/// rather than only that something was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodySizeExceeded {
    /// The maximum body size, in bytes.
    pub limit: u64,
}

impl IntoResponse for BodySizeExceeded {
    fn into_response(self) -> http::Response {
        Problem::new(http::StatusCode::PAYLOAD_TOO_LARGE)
            .with_detail(format!("the request body exceeds {} bytes", self.limit))
            .into_response()
    }
}

impl ShortCircuit for BodySizeExceeded {
    const STATUSES: &'static [u16] = &[413];
}

impl Responses for BodySizeExceeded {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        kynos_openapi::Responses::new().with(
            413,
            kynos_openapi::Response::new("the request body exceeds the configured limit"),
        )
    }
}

/// Caps the size of a request body.
///
/// Contributes 413 to every covered operation — which is the point.
/// Configuring a limit and documenting that the limit exists are the same
/// action, so an API cannot quietly reject payloads it claims to accept.
#[derive(Clone, Copy, Debug)]
pub struct BodySize {
    /// The maximum body size, in bytes.
    pub limit: u64,
}

impl BodySize {
    /// Caps bodies at `bytes`.
    #[must_use]
    pub fn new(bytes: u64) -> Self {
        Self { limit: bytes }
    }
}

/// The length the request declared, when it declared one.
fn declared_length(headers: &http::HeaderMap) -> Option<u64> {
    headers
        .get(http::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Reads `body` while the running total stays within `limit`.
///
/// `None` once the limit is passed, which is decided on the frame that passes
/// it rather than after the whole body has arrived — a chunked body declares no
/// length, so the count is the only bound there is.
///
/// A read that fails yields what arrived before it did. That is not a size
/// violation and must not be reported as one; the body extractor beneath sees a
/// truncated payload and rejects it with the status it already describes.
async fn read_capped(mut body: crate::http::body::Body, limit: u64) -> Option<Bytes> {
    let mut collected = BytesMut::new();

    while let Some(frame) = body.frame().await {
        let Ok(frame) = frame else { break };
        let Ok(data) = frame.into_data() else {
            continue;
        };

        let collected_so_far = u64::try_from(collected.len()).unwrap_or(u64::MAX);
        let arriving = u64::try_from(data.len()).unwrap_or(u64::MAX);
        if collected_so_far.saturating_add(arriving) > limit {
            return None;
        }

        collected.extend_from_slice(&data);
    }

    Some(collected.freeze())
}

impl<C: Sync + 'static> Interceptor<C> for BodySize {
    type Reads = ();
    type Adds = ();
    type Short = BodySizeExceeded;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, BodySizeExceeded> {
        let _ = (reads, context);

        // A declared length is the cheapest answer: an oversized upload is
        // refused before a byte of it is read.
        if let Some(declared) = declared_length(request.headers()) {
            if declared > self.limit {
                return Err(BodySizeExceeded { limit: self.limit });
            }

            // The protocol driver delivers no more than the length it was told,
            // so the body passes through untouched and a streaming upload stays
            // one.
            return Ok(next.run(request).await);
        }

        // No declared length, so the count is the only bound: the body is read
        // frame by frame and abandoned the moment it passes the limit. What
        // arrives within it is handed on verbatim, since the only body Kynos can
        // rebuild is one built from bytes.
        let (parts, body) = request.into_parts();
        let Some(bytes) = read_capped(body, self.limit).await else {
            return Err(BodySizeExceeded { limit: self.limit });
        };

        let request = http::Request::from_parts(parts, crate::http::body::Body::from_bytes(bytes));
        Ok(next.run(request).await)
    }
}

/// What [`Timeout`] answers with when a handler runs too long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedOut {
    /// The limit the handler passed.
    pub after: Duration,
}

impl IntoResponse for TimedOut {
    fn into_response(self) -> http::Response {
        Problem::new(http::StatusCode::GATEWAY_TIMEOUT)
            .with_detail(format!(
                "the handler did not finish within {} seconds",
                self.after.as_secs()
            ))
            .into_response()
    }
}

impl ShortCircuit for TimedOut {
    const STATUSES: &'static [u16] = &[504];
}

impl Responses for TimedOut {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        kynos_openapi::Responses::new().with(
            504,
            kynos_openapi::Response::new("the handler did not finish within the configured limit"),
        )
    }
}

/// Caps how long a handler may run.
///
/// Contributes 504.
#[derive(Clone, Copy, Debug)]
pub struct Timeout {
    /// The maximum handler duration.
    pub limit: std::time::Duration,
}

impl Timeout {
    /// Limits handlers to `limit`.
    pub fn new(limit: std::time::Duration) -> Self {
        Self { limit }
    }
}

impl<C: Sync + 'static> Interceptor<C> for Timeout {
    type Reads = ();
    type Adds = ();
    type Short = TimedOut;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, TimedOut> {
        let _ = (request, reads, context, next, self.limit);
        todo!()
    }
}

/// What [`Concurrency`] answers with when every slot is taken.
///
/// The `Retry-After` header is *this type's*, not a separate entry keyed on
/// 503: the type that sets the header is the type that describes it, so the two
/// cannot come apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtCapacity {
    /// How long a client should wait, when there is a useful answer.
    pub retry_after: Option<Duration>,
}

impl IntoResponse for AtCapacity {
    fn into_response(self) -> http::Response {
        let mut response = Problem::new(http::StatusCode::SERVICE_UNAVAILABLE)
            .with_detail("the service is at its concurrency limit")
            .into_response();
        set_retry_after(&mut response, self.retry_after);
        response
    }
}

impl ShortCircuit for AtCapacity {
    const STATUSES: &'static [u16] = &[503];
}

impl Responses for AtCapacity {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        kynos_openapi::Responses::new().with(
            503,
            kynos_openapi::Response::new("the service is at its concurrency limit")
                .with_header("Retry-After", retry_after_header()),
        )
    }
}

/// Caps concurrent in-flight requests.
///
/// Contributes 503 and a `Retry-After` response header.
#[derive(Clone, Copy, Debug)]
pub struct Concurrency {
    /// The maximum number of requests in flight at once.
    pub limit: usize,
}

impl Concurrency {
    /// Limits in-flight requests to `limit`.
    pub fn new(limit: usize) -> Self {
        Self { limit }
    }
}

impl<C: Sync + 'static> Interceptor<C> for Concurrency {
    type Reads = ();
    type Adds = ();
    type Short = AtCapacity;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, AtCapacity> {
        let _ = (request, reads, context, next, self.limit);
        todo!()
    }
}
