//! A cap on the size of a request body, and the 413 it answers with.

use std::{fmt, marker::PhantomData};

use bytes::BytesMut;
use http_body_util::BodyExt;

use crate::{
    error::problem::{ProblemType, refusal_problem, refusal_response},
    extract::body::limit::{BodyLimit, declared_length},
    http::{self, body::Body},
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// What [`BodySize`] answers with when a body is too large.
///
/// Carries the limit it enforced, so the response can say what was exceeded
/// rather than only that something was.
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`. Set
/// it with [`BodySize::problem_type`].
pub struct BodySizeExceeded<T = ()> {
    /// The maximum body size, in bytes.
    pub limit: u64,
    /// Carries `T` without storing one; `fn() -> T` keeps it `Send` and `Sync`.
    problem_type: PhantomData<fn() -> T>,
}

impl<T> BodySizeExceeded<T> {
    /// A refusal reporting `limit` as the ceiling that was exceeded.
    #[must_use]
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            problem_type: PhantomData,
        }
    }
}

impl<T: ProblemType> IntoResponse for BodySizeExceeded<T> {
    fn into_response(self) -> http::Response {
        refusal_problem::<T>(http::StatusCode::PAYLOAD_TOO_LARGE)
            .with_detail(format!("the request body exceeds {} bytes", self.limit))
            .into_response()
    }
}

impl<T: ProblemType> ShortCircuit for BodySizeExceeded<T> {
    const STATUSES: &'static [u16] = &[413];
}

impl<T: ProblemType> Responses for BodySizeExceeded<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            413,
            refusal_response::<T>(
                registry,
                413,
                "the request body exceeds the configured limit",
            ),
        )
    }
}

/// Caps the size of a request body.
///
/// Contributes 413 to every covered operation, so configuring a limit also
/// documents it.
///
/// # Replacing the default
///
/// Every body extractor that buffers already caps what it reads at
/// [`DEFAULT_LIMIT`](crate::extract::body::limit::DEFAULT_LIMIT). This limit
/// replaces that one for every operation it covers, in either direction, so
/// mounting it on a single endpoint is how one large upload is let through:
/// `kynos::routes![upload.intercept(BodySize::new(..))]` for an attribute
/// route. It also covers operations that read no body, which then declare a
/// 413 too; mount it where bodies are read.
///
/// # What it costs a streaming read
///
/// A request declaring a `Content-Length` is decided from the head, and the
/// body passes through untouched: a streaming extractor such as
/// [`Records`](crate::extract::body::json_lines::records::Records) still receives it a
/// frame at a time. A chunked request declares no length, so the running count
/// is the only bound there is and the whole body is materialised here before
/// the handler is entered, so the 413 precedes any side effect of the payload.
/// Records then still arrive one at a time, but the memory the streaming was
/// for has already been spent.
///
/// # Naming what the 413 is
///
/// [`problem_type`](BodySize::problem_type) puts an application's own URI on
/// the refusal, so a client can tell an oversized upload from every other 413
/// the service sends. See [`ProblemType`] for why it is a type and not a value.
pub struct BodySize<T = ()> {
    /// The maximum body size, in bytes.
    pub limit: u64,
    /// Names the refusal's problem type without holding one.
    problem_type: PhantomData<fn() -> T>,
}

impl BodySize<()> {
    /// Caps bodies at `bytes`.
    #[must_use]
    pub fn new(bytes: u64) -> Self {
        Self {
            limit: bytes,
            problem_type: PhantomData,
        }
    }

    /// Names the RFC 9457 problem type this limit's 413 carries.
    ///
    /// Changes the type, because it changes what every covered operation
    /// declares; both the response body and the description read it.
    ///
    /// Available only on a limit that has not named one, so a chain states the
    /// type at most once.
    ///
    /// ```
    /// use kynos::{error::problem::ProblemType, middleware::limits::body_size::BodySize};
    ///
    /// struct PayloadTooLarge;
    ///
    /// impl ProblemType for PayloadTooLarge {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/too-large");
    /// }
    ///
    /// let limit = BodySize::new(1_024).problem_type::<PayloadTooLarge>();
    /// # let _ = limit;
    /// ```
    ///
    /// Naming a second one does not compile:
    ///
    /// ```compile_fail
    /// use kynos::{error::problem::ProblemType, middleware::limits::body_size::BodySize};
    ///
    /// struct PayloadTooLarge;
    /// # impl ProblemType for PayloadTooLarge {
    /// #     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/too-large");
    /// # }
    /// struct Overweight;
    /// # impl ProblemType for Overweight {
    /// #     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/overweight");
    /// # }
    ///
    /// let limit = BodySize::new(1_024)
    ///     .problem_type::<PayloadTooLarge>()
    ///     .problem_type::<Overweight>();
    /// # let _ = limit;
    /// ```
    #[must_use]
    pub fn problem_type<T: ProblemType>(self) -> BodySize<T> {
        BodySize {
            limit: self.limit,
            problem_type: PhantomData,
        }
    }
}

/// Reads `body` while the running total stays within `limit`; `None` on the
/// frame that passes it.
///
/// A failed read is handed on as it failed (bytes so far, then the error), so
/// the extractor beneath refuses it rather than parsing a truncated payload.
async fn read_capped(mut body: Body, limit: u64) -> Option<Body> {
    let mut collected = BytesMut::new();

    while let Some(frame) = body.frame().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                return Some(Body::failed_after(collected.freeze(), error));
            }
        };
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

    Some(Body::from_bytes(collected.freeze()))
}

impl<C, T> Interceptor<C> for BodySize<T>
where
    C: Sync + 'static,
    T: ProblemType,
{
    type Reads = ();
    type Adds = ();
    type Short = BodySizeExceeded<T>;

    async fn intercept(
        &self,
        mut request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, BodySizeExceeded<T>> {
        let _ = (reads, context);

        // Replaces the extractor's default in both directions, so an extractor
        // beneath never refuses what was let through here.
        request.extensions_mut().insert(BodyLimit(self.limit));

        if let Some(declared) = declared_length(request.headers()) {
            if declared > self.limit {
                return Err(BodySizeExceeded::new(self.limit));
            }

            // The protocol driver delivers no more than the declared length.
            return Ok(next.run(request).await);
        }

        let (parts, body) = request.into_parts();
        let Some(body) = read_capped(body, self.limit).await else {
            return Err(BodySizeExceeded::new(self.limit));
        };

        let request = http::Request::from_parts(parts, body);
        Ok(next.run(request).await)
    }
}

// Not derived: a derive would bound each on the marker. Each destructures
// `self`, so a new field is a compile error here.

impl<T> Clone for BodySizeExceeded<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for BodySizeExceeded<T> {}

impl<T> fmt::Debug for BodySizeExceeded<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            limit,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("BodySizeExceeded")
            .field("limit", limit)
            .finish()
    }
}

impl<T> PartialEq for BodySizeExceeded<T> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            limit,
            problem_type: _,
        } = self;

        *limit == other.limit
    }
}

impl<T> Eq for BodySizeExceeded<T> {}

impl<T> Clone for BodySize<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for BodySize<T> {}

impl<T> fmt::Debug for BodySize<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            limit,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("BodySize")
            .field("limit", limit)
            .finish()
    }
}
