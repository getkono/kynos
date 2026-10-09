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
    /// Carries `T` without storing one. `fn() -> T` rather than `T`, so a
    /// refusal is `Send` and `Sync` whatever the marker is.
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
/// Contributes 413 to every covered operation — which is the point.
/// Configuring a limit and documenting that the limit exists are the same
/// action, so an API cannot quietly reject payloads it claims to accept.
///
/// # Replacing the default
///
/// Every body extractor that buffers already caps what it reads at
/// [`DEFAULT_LIMIT`](crate::extract::body::limit::DEFAULT_LIMIT). This limit
/// replaces that one for every operation it covers, in either direction, so
/// mounting it on a single endpoint is how one large upload is let through:
/// `kynos::routes![upload].0.intercept(BodySize::new(..))` for an attribute
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
/// the handler is entered. Records then still arrive one at a time, but the
/// memory the streaming was for has already been spent.
///
/// That follows from what the declared 413 promises, not from what [`Body`] can
/// be built from. A count that runs while the handler reads reaches its verdict
/// only after the handler has acted on the bytes it was given, so streaming
/// here would not restore the cap — it would move the refusal behind whatever
/// an oversized payload had already caused. The alternatives are a 413 sent
/// after those side effects, or a 411 refusing every length-less body and with
/// it every chunked upload; both are worse trades than the buffer.
/// `docs/nfr.md` records the same conclusion, and there is no missing
/// constructor to write.
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
    ///
    /// Declared on the concrete type rather than on the generic one so that
    /// this still infers without a turbofish: a default type parameter does not
    /// participate in inference from an associated function.
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
    /// declares. Stated once, and read by both the response body and the
    /// description.
    ///
    /// Available only on a limit that has not named one, so a chain states the
    /// type at most once and a reader never has to find the last call that won.
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
    /// Naming a second one does not compile — the `impl` block is on
    /// `BodySize<()>`, so the method is simply not there once `T` is a type.
    /// The block above is this rule's pass control: the two differ only in the
    /// second call.
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

/// Reads `body` while the running total stays within `limit`, returning the
/// body to hand on.
///
/// `None` once the limit is passed, which is decided on the frame that passes
/// it rather than after the whole body has arrived — a chunked body declares no
/// length, so the count is the only bound there is.
///
/// A read that fails is handed on as it failed — the bytes that arrived, then
/// the same error — so the extractor beneath refuses it with the status it
/// already describes, whatever that extractor parses. Swallowing the error
/// would hand a truncated payload to one that parses nothing.
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

        // This limit replaces the extractor's default for every operation it
        // covers, upward as well as downward: an extractor beneath reads the
        // body under the same figure this enforces, so it never refuses what
        // was let through here.
        request.extensions_mut().insert(BodyLimit(self.limit));

        // A declared length is the cheapest answer: an oversized upload is
        // refused before a byte of it is read.
        if let Some(declared) = declared_length(request.headers()) {
            if declared > self.limit {
                return Err(BodySizeExceeded::new(self.limit));
            }

            // The protocol driver delivers no more than the length it was told,
            // so the body passes through untouched and a streaming upload stays
            // one.
            return Ok(next.run(request).await);
        }

        // No declared length, so the count is the only bound: the body is read
        // frame by frame and abandoned the moment it passes the limit. What
        // arrives within it is handed on verbatim — a failure included — since
        // the only body Kynos can rebuild is one built from what was read.
        let (parts, body) = request.into_parts();
        let Some(body) = read_capped(body, self.limit).await else {
            return Err(BodySizeExceeded::new(self.limit));
        };

        let request = http::Request::from_parts(parts, body);
        Ok(next.run(request).await)
    }
}

// --- The derivable implementations, written out ---------------------------
//
// `#[derive]` would bound each on the marker, and a marker is a name rather
// than a value: it is never cloned, printed or compared, and requiring it to be
// would make naming a problem type cost four derives on the application's own
// marker. Every one destructures `self`, so a field added to a refusal is a
// compile error here rather than a member these silently stop reading. The
// other limits that carry a marker write theirs out for the same reason.

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

// The interceptor carries the same parameter for the same reason: derived, a
// limit naming a problem type would lose `Clone` and `Debug` unless the
// application's marker derived them too.

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
