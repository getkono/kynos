//! Limits, and the responses they make possible.
//!
//! Each limit here owns a type for the response it answers with. That is what
//! keeps the declaration and the behaviour one fact rather than two: the status
//! a limit can produce is the status its response type describes, and a header
//! that rides that status — `Retry-After` on a 503 — is described by the same
//! type that sets it, rather than by a separate entry keyed on the status.

use std::{fmt, marker::PhantomData, num::NonZeroUsize, sync::Arc, time::Duration};

use bytes::{Bytes, BytesMut};
use http_body_util::BodyExt;
use kynos_openapi::model::schema::types::SchemaType;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::{
    error::problem::{ProblemType, refusal_problem, refusal_response},
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
/// That follows from what the declared 413 promises, not from what
/// [`Body`](crate::http::body::Body) can be built from. A count that runs while
/// the handler reads reaches its verdict only after the handler has acted on
/// the bytes it was given, so streaming here would not restore the cap — it
/// would move the refusal behind whatever an oversized payload had already
/// caused. The alternatives are a 413 sent after those side effects, or a 411
/// refusing every length-less body and with it every chunked upload; both are
/// worse trades than the buffer. `docs/nfr.md` records the same conclusion, and
/// there is no missing constructor to write.
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
    /// use kynos::{error::problem::ProblemType, middleware::limits::BodySize};
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
    /// use kynos::{error::problem::ProblemType, middleware::limits::BodySize};
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
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, BodySizeExceeded<T>> {
        let _ = (reads, context);

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
        // arrives within it is handed on verbatim, since the only body Kynos can
        // rebuild is one built from bytes.
        let (parts, body) = request.into_parts();
        let Some(bytes) = read_capped(body, self.limit).await else {
            return Err(BodySizeExceeded::new(self.limit));
        };

        let request = http::Request::from_parts(parts, crate::http::body::Body::from_bytes(bytes));
        Ok(next.run(request).await)
    }
}

/// What [`Timeout`] answers with when a handler runs too long.
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`. Set
/// it with [`Timeout::problem_type`], or replace this type outright with
/// [`Timeout::answer_with`].
pub struct TimedOut<T = ()> {
    /// The limit the handler passed.
    pub after: Duration,
    /// Carries `T` without storing one, as in [`BodySizeExceeded`].
    problem_type: PhantomData<fn() -> T>,
}

impl<T> TimedOut<T> {
    /// A refusal reporting `after` as the limit the handler passed.
    #[must_use]
    pub fn new(after: Duration) -> Self {
        Self {
            after,
            problem_type: PhantomData,
        }
    }
}

impl<T: ProblemType> IntoResponse for TimedOut<T> {
    fn into_response(self) -> http::Response {
        refusal_problem::<T>(http::StatusCode::REQUEST_TIMEOUT)
            .with_detail(format!(
                "the handler did not finish within {} seconds",
                self.after.as_secs()
            ))
            .into_response()
    }
}

impl<T: ProblemType> ShortCircuit for TimedOut<T> {
    const STATUSES: &'static [u16] = &[408];
}

impl<T: ProblemType> Responses for TimedOut<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            408,
            refusal_response::<T>(
                registry,
                408,
                "the handler did not finish within the configured limit",
            ),
        )
    }
}

/// Caps how long a handler may run.
///
/// Contributes 408.
///
/// # Why 408 and not 504
///
/// RFC 9110 section 15.6.5 scopes 504 to a server "while acting as a gateway or
/// proxy" awaiting "an upstream server it needed to access". Kynos is an origin
/// and this interceptor wraps its own chain, so every clause of that definition
/// is false — and 504 is a status a load balancer or CDN in front of the service
/// genuinely sends, which made an origin's own indistinguishable from that hop's
/// in logs and in client retry logic.
///
/// 408 is not exact either. Section 15.5.9 defines it as the server not having
/// received "a complete request message within the time that it was prepared to
/// wait", which describes the slow-body arrangement below precisely and the
/// handler-runtime case only by extension. It is the closest status the
/// specification defines, it carries a retry semantic clients already implement,
/// and it is what `tower-http` sends for the same situation.
///
/// 503 would have read better for handler runtime — "temporary overload" — and
/// is not available: [`Concurrency`] declares it, so `statuses_disjoint` would
/// refuse a router carrying both. Bounding handler time *and* capping
/// concurrency is an ordinary pairing, and a status choice that made it
/// uncompilable would be a worse answer than an inexact one.
///
/// # Mount it outside a [`BodySize`]
///
/// A timeout wraps whatever is beneath it, so it bounds a body read only when
/// it is the *earlier* `intercept` call, per
/// [the module's ordering rule](super#the-order-a-chain-runs-in). [`BodySize`]
/// walks a length-less body frame by frame, and a client that sends one frame
/// slowly holds that loop open with nothing above it to end the exchange.
///
/// Nothing enforces this. `CompatibleWith` compares sets, and a set has no
/// positions, so the wrong order compiles and describes itself identically.
///
/// # Answering with something else
///
/// The response type is a parameter, defaulting to [`TimedOut`]. Reach for
/// [`answer_with`](Timeout::answer_with) when a timeout should carry more than
/// a status and a sentence — a support identifier, a `Retry-After`, a
/// diagnostic an operator can correlate — or when the whole service answers
/// timeouts in a house-specific shape.
///
/// The substitute is a [`ShortCircuit`], so it still declares the statuses it
/// can produce and still contributes them to every operation the interceptor
/// covers. A custom response cannot make the document wrong: whatever it
/// answers with, `statuses_disjoint` sees the same `STATUSES` the compiler
/// checks against every other interceptor in the stack.
///
/// ```no_run
/// use std::time::Duration;
/// # use kynos::{
/// #     http, middleware::limits::Timeout, response::{IntoResponse, Responses, ShortCircuit},
/// #     schema::registry::Registry,
/// # };
/// /// What this service answers a timeout with.
/// struct TookTooLong {
///     after: Duration,
/// }
///
/// impl From<Duration> for TookTooLong {
///     fn from(after: Duration) -> Self {
///         // The one place to emit a warning, a metric or a trace event: it
///         // runs exactly when the handler was abandoned.
///         eprintln!("abandoned a handler after {after:?}");
///         Self { after }
///     }
/// }
/// # impl IntoResponse for TookTooLong {
/// #     fn into_response(self) -> http::Response { todo!() }
/// # }
/// # impl Responses for TookTooLong {
/// #     fn responses(registry: &mut Registry) -> kynos_openapi::Responses { todo!() }
/// # }
/// # impl ShortCircuit for TookTooLong { const STATUSES: &'static [u16] = &[408]; }
/// let timeout = Timeout::new(Duration::from_secs(30)).answer_with::<TookTooLong>();
/// # let _ = timeout;
/// ```
pub struct Timeout<R = TimedOut<()>> {
    /// The maximum handler duration.
    pub limit: std::time::Duration,
    /// Names the response without holding one.
    ///
    /// `fn() -> R` so that `R` decides nothing about this type's auto traits:
    /// a `Timeout` is `Send` because a `Duration` is.
    _response: PhantomData<fn() -> R>,
}

impl Timeout<TimedOut<()>> {
    /// Limits handlers to `limit`.
    ///
    /// Answers with [`TimedOut`]. Declared on the concrete type rather than on
    /// the generic one so that this still infers without a turbofish: a default
    /// type parameter does not participate in inference from an associated
    /// function.
    #[must_use]
    pub fn new(limit: std::time::Duration) -> Self {
        Self {
            limit,
            _response: PhantomData,
        }
    }

    /// Names the RFC 9457 problem type this timeout's 408 carries.
    ///
    /// The smaller half of [`answer_with`](Timeout::answer_with): this names
    /// the type a [`TimedOut`] publishes and changes nothing else, where
    /// `answer_with` replaces the response outright. Reach for that one when a
    /// timeout owes more than a URI — a `Retry-After`, a support identifier, a
    /// shape the whole service answers timeouts in.
    ///
    /// Available only on a timeout still answering with an unnamed
    /// [`TimedOut`], so a chain states the type at most once. See
    /// [`BodySize::problem_type`] for the rule and its pass control.
    ///
    /// ```
    /// # use std::time::Duration;
    /// use kynos::{error::problem::ProblemType, middleware::limits::Timeout};
    ///
    /// struct TookTooLong;
    ///
    /// impl ProblemType for TookTooLong {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/timed-out");
    /// }
    ///
    /// let timeout = Timeout::new(Duration::from_secs(30)).problem_type::<TookTooLong>();
    /// # let _ = timeout;
    /// ```
    #[must_use]
    pub fn problem_type<T: ProblemType>(self) -> Timeout<TimedOut<T>> {
        Timeout {
            limit: self.limit,
            _response: PhantomData,
        }
    }
}

impl<R> Timeout<R> {
    /// Answers timeouts with `S` instead of [`TimedOut`].
    ///
    /// `S` is built from the limit that elapsed, so `From<Duration>` is where a
    /// warning, a metric or a trace event belongs: it runs exactly when a
    /// handler is abandoned, which is the moment nothing else observes.
    #[must_use]
    pub fn answer_with<S>(self) -> Timeout<S>
    where
        S: ShortCircuit + From<std::time::Duration> + Send + 'static,
    {
        Timeout {
            limit: self.limit,
            _response: PhantomData,
        }
    }
}

// Hand-written rather than derived: a derive would bound `R: Clone` and
// `R: Debug`, and `PhantomData<fn() -> R>` needs neither.
impl<R> Clone for Timeout<R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R> Copy for Timeout<R> {}

impl<R> std::fmt::Debug for Timeout<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Timeout")
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

impl<C, R> Interceptor<C> for Timeout<R>
where
    C: Sync + 'static,
    R: ShortCircuit + From<std::time::Duration> + Send + 'static,
{
    type Reads = ();
    type Adds = ();
    type Short = R;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, R> {
        let _ = (reads, context);

        // The timer is the one thing this cannot do for itself. Dropping the
        // chain's future is what stops the handler: there is no other way to
        // abandon work that is already running.
        match tokio::time::timeout(self.limit, next.run(request)).await {
            Ok(continued) => Ok(continued),
            Err(_elapsed) => Err(R::from(self.limit)),
        }
    }
}

impl<T> From<Duration> for TimedOut<T> {
    fn from(after: Duration) -> Self {
        Self::new(after)
    }
}

/// What [`Concurrency`] answers with when every slot is taken.
///
/// The `Retry-After` header is *this type's*, not a separate entry keyed on
/// 503: the type that sets the header is the type that describes it, so the two
/// cannot come apart.
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`. Set
/// it with [`Concurrency::problem_type`], which is the one URI away that tells
/// a shed 503 from every other 503 a service can send.
pub struct AtCapacity<T = ()> {
    /// How long a client should wait, when there is a useful answer.
    pub retry_after: Option<Duration>,
    /// Carries `T` without storing one, as in [`BodySizeExceeded`].
    problem_type: PhantomData<fn() -> T>,
}

impl<T> AtCapacity<T> {
    /// A refusal advertising `retry_after`, where there is a useful answer.
    #[must_use]
    pub fn new(retry_after: Option<Duration>) -> Self {
        Self {
            retry_after,
            problem_type: PhantomData,
        }
    }
}

impl<T: ProblemType> IntoResponse for AtCapacity<T> {
    fn into_response(self) -> http::Response {
        let mut response = refusal_problem::<T>(http::StatusCode::SERVICE_UNAVAILABLE)
            .with_detail("the service is at its concurrency limit")
            .into_response();
        set_retry_after(&mut response, self.retry_after);
        response
    }
}

impl<T: ProblemType> ShortCircuit for AtCapacity<T> {
    const STATUSES: &'static [u16] = &[503];
}

impl<T: ProblemType> Responses for AtCapacity<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            503,
            refusal_response::<T>(registry, 503, "the service is at its concurrency limit")
                .with_header("Retry-After", retry_after_header()),
        )
    }
}

/// Caps concurrent in-flight requests.
///
/// Contributes 503 and a `Retry-After` response header.
///
/// Requests are shed rather than queued by default: a queue is a delay a client
/// cannot see, and 503 is the answer [`AtCapacity`] describes.
/// [`queue_for`](Concurrency::queue_for) makes the wait bounded and explicit for
/// a deployment that would rather absorb a burst than refuse it.
///
/// Cloning shares the permits, so one limit stays one limit however many copies
/// the router holds — and mounting a *separate* instance on each endpoint is
/// how one cap per endpoint is spelled.
///
/// # A limit of zero is not a limit
///
/// It is a service that answers 503 to everything, for ever, without saying so
/// anywhere. The limit is therefore a [`NonZeroUsize`], which is the same
/// spelling [`Server::max_connections`](crate::server::Server::max_connections)
/// uses for the same concept:
///
/// ```
/// # use std::num::NonZeroUsize;
/// # use kynos::middleware::limits::Concurrency;
/// let concurrency = Concurrency::new(NonZeroUsize::new(64).expect("nonzero"));
/// assert_eq!(concurrency.limit.get(), 64);
/// ```
///
/// Zero has no `NonZeroUsize` to be, so the mistake does not compile:
///
/// ```compile_fail
/// # use kynos::middleware::limits::Concurrency;
/// let concurrency = Concurrency::new(0);
/// ```
///
/// # Naming what the 503 is
///
/// A 503 from a concurrency cap and a 503 from anything else are one URI apiece
/// away from being distinguishable, and
/// [`problem_type`](Concurrency::problem_type) is that URI.
pub struct Concurrency<T = ()> {
    /// The maximum number of requests in flight at once.
    pub limit: NonZeroUsize,
    slots: Arc<Semaphore>,
    queue_for: Duration,
    retry_after: Option<Duration>,
    /// Names the refusal's problem type without holding one.
    problem_type: PhantomData<fn() -> T>,
}

impl Concurrency<()> {
    /// Limits in-flight requests to `limit`.
    ///
    /// Declared on the concrete type so that it still infers without a
    /// turbofish, as [`BodySize::new`] is.
    #[must_use]
    pub fn new(limit: NonZeroUsize) -> Self {
        Self {
            limit,
            slots: Arc::new(Semaphore::new(limit.get())),
            queue_for: Duration::ZERO,
            retry_after: None,
            problem_type: PhantomData,
        }
    }

    /// Names the RFC 9457 problem type this cap's 503 carries.
    ///
    /// Available only on a cap that has not named one, so a chain states the
    /// type at most once. See [`BodySize::problem_type`] for the rule and its
    /// pass control.
    ///
    /// ```
    /// # use std::num::NonZeroUsize;
    /// use kynos::{error::problem::ProblemType, middleware::limits::Concurrency};
    ///
    /// struct Shed;
    ///
    /// impl ProblemType for Shed {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/shed");
    /// }
    ///
    /// let concurrency = Concurrency::new(NonZeroUsize::new(64).expect("nonzero"))
    ///     .problem_type::<Shed>();
    /// # let _ = concurrency;
    /// ```
    #[must_use]
    pub fn problem_type<T: ProblemType>(self) -> Concurrency<T> {
        Concurrency {
            limit: self.limit,
            slots: self.slots,
            queue_for: self.queue_for,
            retry_after: self.retry_after,
            problem_type: PhantomData,
        }
    }
}

impl<T> Concurrency<T> {
    /// Waits up to `wait` for a slot before shedding.
    ///
    /// Declares nothing new. The answer when the wait expires is the same 503,
    /// and a delay is not a response — `Timeout` already changes how long an
    /// exchange takes without contributing a status for the change.
    ///
    /// Zero, the default, sheds immediately.
    #[must_use]
    pub fn queue_for(mut self, wait: Duration) -> Self {
        self.queue_for = wait;
        self
    }

    /// The `Retry-After` a shed response carries.
    ///
    /// Absent by default, because how long a slot takes to free is a property
    /// of the requests already running and a number invented here is one the
    /// service cannot honour. A deployment behind an autoscaler *does* know,
    /// which is why this is a value it supplies rather than a guess Kynos makes
    /// — and why [`AtCapacity`] describes the header either way.
    #[must_use]
    pub fn retry_after(mut self, delay: Duration) -> Self {
        self.retry_after = Some(delay);
        self
    }

    /// Takes a slot, waiting no longer than the configured queue.
    ///
    /// An owned permit rather than a counter pair: the chain's future can be
    /// dropped at any await point, and a slot that leaked on cancellation would
    /// shrink the limit until the process restarted.
    async fn acquire(&self) -> Option<OwnedSemaphorePermit> {
        if self.queue_for.is_zero() {
            return Arc::clone(&self.slots).try_acquire_owned().ok();
        }

        tokio::time::timeout(self.queue_for, Arc::clone(&self.slots).acquire_owned())
            .await
            .ok()?
            .ok()
    }
}

impl<C, T> Interceptor<C> for Concurrency<T>
where
    C: Sync + 'static,
    T: ProblemType,
{
    type Reads = ();
    type Adds = ();
    type Short = AtCapacity<T>;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, AtCapacity<T>> {
        let _ = (reads, context);

        let Some(_slot) = self.acquire().await else {
            return Err(AtCapacity::new(self.retry_after));
        };

        Ok(next.run(request).await)
    }
}

// --- The derivable implementations, written out ---------------------------
//
// `#[derive]` would bound each on the marker, and a marker is a name rather
// than a value: it is never cloned, printed or compared, and requiring it to be
// would make naming a problem type cost four derives on the application's own
// marker. Every one destructures `self`, so a field added to a refusal is a
// compile error here rather than a member these silently stop reading.

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

impl<T> Clone for TimedOut<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for TimedOut<T> {}

impl<T> fmt::Debug for TimedOut<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            after,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("TimedOut")
            .field("after", after)
            .finish()
    }
}

impl<T> PartialEq for TimedOut<T> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            after,
            problem_type: _,
        } = self;

        *after == other.after
    }
}

impl<T> Eq for TimedOut<T> {}

impl<T> Clone for AtCapacity<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for AtCapacity<T> {}

impl<T> fmt::Debug for AtCapacity<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            retry_after,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("AtCapacity")
            .field("retry_after", retry_after)
            .finish()
    }
}

impl<T> PartialEq for AtCapacity<T> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            retry_after,
            problem_type: _,
        } = self;

        *retry_after == other.retry_after
    }
}

impl<T> Eq for AtCapacity<T> {}

// The interceptors carry the same parameter for the same reason: derived, a
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

impl<T> Clone for Concurrency<T> {
    fn clone(&self) -> Self {
        let Self {
            limit,
            slots,
            queue_for,
            retry_after,
            problem_type: _,
        } = self;

        Self {
            limit: *limit,
            // Shared, so one limit stays one limit however many copies the
            // router holds.
            slots: Arc::clone(slots),
            queue_for: *queue_for,
            retry_after: *retry_after,
            problem_type: PhantomData,
        }
    }
}

impl<T> fmt::Debug for Concurrency<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            limit,
            slots,
            queue_for,
            retry_after,
            problem_type: _,
        } = self;

        formatter
            .debug_struct("Concurrency")
            .field("limit", limit)
            .field("slots", slots)
            .field("queue_for", queue_for)
            .field("retry_after", retry_after)
            .finish()
    }
}

/// The error a body bounded by [`BodyTimeout`] ends with.
///
/// Reaches a client as a truncated response and nothing else: the status and
/// the headers left before the timer did, so there is no status left to send.
/// It is an error rather than a clean end so that the protocol driver resets
/// the stream instead of framing the truncation as a complete body — a
/// consumer that reads a length or a terminating chunk has to be able to tell
/// the two apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyTimedOut {
    /// The limit the body passed.
    pub after: Duration,
}

impl std::fmt::Display for BodyTimedOut {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the response body did not finish within {:?}",
            self.after
        )
    }
}

impl std::error::Error for BodyTimedOut {}

/// Caps how long a response body may take.
///
/// Declares no status, and cannot: by the time a body is streaming, the status
/// and the headers have already left. What this bounds is the part of a
/// response [`Timeout`] cannot see.
///
/// # Why `Timeout` does not already cover this
///
/// [`Timeout`] wraps the chain's future, and that future completes when the
/// *head* is ready. A handler returning a stream — Server-Sent Events, JSON
/// Lines, a large body read from elsewhere — returns immediately and then
/// emits for as long as it likes. Its timer has already stopped by then, so a
/// handler that never finishes streaming is bounded by nothing.
///
/// # Idle, or a deadline
///
/// [`idle`](BodyTimeout::idle) restarts the clock on every frame, so it bounds
/// the *gap* between frames and catches a peer or an upstream that stopped
/// producing. [`deadline`](BodyTimeout::deadline) never restarts it, so it
/// bounds the total time a body may take however steadily it arrives.
///
/// Idle is the one to reach for by default. A deadline over a long-lived
/// stream ends a healthy response for being long, which is rarely what an
/// operator means; it earns its place over a body with a bounded size, where
/// exceeding a wall-clock budget really is a fault.
///
/// # Mount it outside anything that rewrites a body
///
/// An interceptor that rewrites a response body has to read it, and one that
/// *buffers* -- [`Compression`](super::compression::Compression) below its size
/// threshold, a cache storing a response -- reads to the end before writing
/// anything. Handed a body that fails part-way, those paths have no partial
/// response to emit and fall back to an empty one, so a timeout mounted beneath
/// them reaches the client as a complete, zero-length success.
///
/// Outside them, the error is the body's last frame and the protocol driver
/// resets the stream, which is what a client needs to see. Nothing enforces
/// this: `CompatibleWith` compares sets, and a set has no positions.
///
/// # Server-Sent Events reset an idle timer
///
/// A keep-alive is a real frame, so it restarts an idle clock exactly as an
/// event does. An event stream with keep-alive enabled and an interval shorter
/// than `limit` therefore never trips one, which is the intended reading — the
/// connection is demonstrably alive — but it does mean `idle` bounds the
/// transport rather than the application there. Use `deadline` to bound how
/// long such a stream may run at all.
#[derive(Clone, Copy, Debug)]
pub struct BodyTimeout {
    /// The maximum gap, or the maximum total, depending on `reset_each_frame`.
    limit: Duration,
    /// Whether a frame restarts the clock.
    reset_each_frame: bool,
}

impl BodyTimeout {
    /// Ends a body that goes `limit` without producing a frame.
    #[must_use]
    pub fn idle(limit: Duration) -> Self {
        Self {
            limit,
            reset_each_frame: true,
        }
    }

    /// Ends a body that has not finished within `limit` of the response head.
    #[must_use]
    pub fn deadline(limit: Duration) -> Self {
        Self {
            limit,
            reset_each_frame: false,
        }
    }
}

impl<C: Sync + 'static> Interceptor<C> for BodyTimeout {
    type Reads = ();
    type Adds = ();
    // No status: the head is already gone when this fires, so there is nothing
    // for an operation to describe and nothing for `statuses_disjoint` to
    // collide with.
    type Short = std::convert::Infallible;

    async fn intercept(
        &self,
        request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, std::convert::Infallible> {
        let _ = (reads, context);

        let mut continued = next.run(request).await;
        let body = continued.take_body();

        continued.set_body(http::body::Body::from_body(Bounded {
            inner: body,
            timer: Box::pin(tokio::time::sleep(self.limit)),
            limit: self.limit,
            reset_each_frame: self.reset_each_frame,
            spent: false,
        }));

        Ok(continued)
    }
}

/// A body that ends if its timer does first.
///
/// The timer is boxed so this needs no projection: `Pin<Box<Sleep>>` is
/// [`Unpin`] whatever `Sleep` is, and `unsafe` is forbidden here. That is the
/// same reason the streamed body boxes its stream.
struct Bounded {
    inner: http::body::Body,
    timer: std::pin::Pin<Box<tokio::time::Sleep>>,
    limit: Duration,
    /// Whether a frame restarts the clock.
    reset_each_frame: bool,
    /// Set once the timer has fired, so the error is yielded exactly once and
    /// a driver that polls again gets the end of the body rather than a second
    /// copy of it.
    spent: bool,
}

impl http_body::Body for Bounded {
    type Data = Bytes;
    type Error = crate::http::body::BoxError;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        use std::task::Poll;

        let this = self.get_mut();

        if this.spent {
            return Poll::Ready(None);
        }

        // A body that has already ended was delivered, whatever the clock says.
        // Without this a deadline landing in the window between the last frame
        // and the poll that observes the end would reset a complete response.
        if this.inner.is_end_stream() {
            return Poll::Ready(None);
        }

        if this.reset_each_frame {
            // The inner body first, and the timer only when it has nothing.
            //
            // An idle limit bounds the *producer*, and the gap this timer
            // measures is between polls rather than between frames. A driver
            // that stops asking -- an HTTP/1 write buffer that is full, an
            // HTTP/2 window that is closed, a saturated executor -- stretches
            // the first without the second moving at all, so consulting the
            // clock first would end a body that had a frame ready and report a
            // slow reader as a stalled writer.
            let polled = std::pin::Pin::new(&mut this.inner).poll_frame(context);

            if matches!(polled, Poll::Ready(Some(Ok(_)))) {
                // `checked_add` because `Instant + Duration` panics where
                // `sleep` saturates, and the limit is the caller's number.
                if let Some(next) = tokio::time::Instant::now().checked_add(this.limit) {
                    this.timer.as_mut().reset(next);
                }

                return polled;
            }

            if polled.is_pending() && this.timer.as_mut().poll(context).is_ready() {
                this.spent = true;
                return Poll::Ready(Some(Err(Box::new(BodyTimedOut { after: this.limit }))));
            }

            return polled;
        }

        // A deadline is consulted first, because a body still producing
        // steadily is exactly the case it exists to end.
        if this.timer.as_mut().poll(context).is_ready() {
            this.spent = true;
            return Poll::Ready(Some(Err(Box::new(BodyTimedOut { after: this.limit }))));
        }

        std::pin::Pin::new(&mut this.inner).poll_frame(context)
    }

    // Deliberately *not* `self.spent || ..`. A body this timer destroyed did
    // not end, and saying otherwise is not a cosmetic difference: `Watched`
    // decides `Delivery::Complete` against `Interrupted` by asking exactly this
    // question when it is dropped, so a `true` here would report a killed
    // response as delivered and `Observer::on_disconnect` would never fire for
    // the one event this interceptor exists to produce.
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        // The inner hint stands: a body that may be cut short still declares
        // what it would have sent, and a driver that trusted a shorter hint
        // would frame the truncation as a complete body.
        self.inner.size_hint()
    }
}
