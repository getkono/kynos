//! A cap on the size of a request body, and the 413 it answers with; and,
//! once given a deadline, a cap on how long that body may take to arrive, and
//! the 408 it answers with.

use std::{
    marker::PhantomData,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use tokio::time::{Instant, Sleep};

use crate::{
    error::problem::{IntoProblem, Problem, ProblemType, refusal_problem, refusal_response},
    extract::body::limit::{BodyLimit, collect_capped, declared_length},
    http::{
        self, HeaderValue, Version,
        body::{Body, BoxError},
        header,
    },
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

impl<T: ProblemType> IntoProblem for BodySizeExceeded<T> {
    fn into_problem(self) -> Problem {
        refusal_problem::<T>(http::StatusCode::PAYLOAD_TOO_LARGE)
            .with_detail(format!("the request body exceeds {} bytes", self.limit))
    }

    fn statuses() -> &'static [http::StatusCode] {
        &[http::StatusCode::PAYLOAD_TOO_LARGE]
    }
}

impl<T: ProblemType> IntoResponse for BodySizeExceeded<T> {
    fn into_response(self) -> http::Response {
        self.into_problem().into_response()
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

/// What a [`BodySize`] with a [`deadline`](BodySize::deadline) answers with
/// when a body has not arrived in time: RFC 9110 section 15.5.9's 408.
///
/// `U` names the problem type the body carries; `()` leaves `about:blank`. Set
/// it with [`BodySize::deadline_problem_type`].
///
/// Over HTTP/1 the 408 a deadline answers with carries `Connection: close`, as
/// that section advises, so the client cannot frame the rest of the body it
/// was sending as a new request. One built with [`new`](BodyTooSlow::new)
/// knows no protocol and carries no such field.
pub struct BodyTooSlow<U = ()> {
    /// The deadline the body passed.
    pub after: Duration,
    /// Whether the response closes the connection.
    closes: bool,
    /// Carries `U` without storing one, as [`BodySizeExceeded`] does.
    problem_type: PhantomData<fn() -> U>,
}

impl<U> BodyTooSlow<U> {
    /// A refusal reporting `after` as the deadline the body passed.
    #[must_use]
    pub fn new(after: Duration) -> Self {
        Self {
            after,
            closes: false,
            problem_type: PhantomData,
        }
    }

    /// This refusal, answering the request `version` names.
    fn answering(self, version: Version) -> Self {
        Self {
            closes: version < Version::HTTP_2,
            ..self
        }
    }
}

impl<U: ProblemType> BodyTooSlow<U> {
    /// The problem this refusal sends, without the `Connection` field that
    /// only [`into_response`](IntoResponse::into_response) writes.
    fn problem(&self) -> Problem {
        refusal_problem::<U>(http::StatusCode::REQUEST_TIMEOUT).with_detail(format!(
            "the request body did not arrive within {:?}",
            self.after
        ))
    }
}

impl<U: ProblemType> IntoResponse for BodyTooSlow<U> {
    fn into_response(self) -> http::Response {
        let mut response = self.problem().into_response();
        if self.closes {
            response
                .headers_mut()
                .insert(header::CONNECTION, HeaderValue::from_static("close"));
        }
        response
    }
}

/// What a [`BodySize`] with a [`deadline`](BodySize::deadline) answers with:
/// a body too large, or one too slow.
///
/// `T` names the 413's problem type and `U` the 408's, since one URI cannot
/// name two problems.
#[non_exhaustive]
pub enum BodyRefused<T = (), U = ()> {
    /// The body is larger than the limit: 413.
    TooLarge(BodySizeExceeded<T>),
    /// The body did not arrive within the deadline: 408.
    TooSlow(BodyTooSlow<U>),
}

/// The problem either refusal sends. A held [`BodyTooSlow`]'s
/// `Connection: close` is a header, so only
/// [`into_response`](IntoResponse::into_response) writes it.
impl<T: ProblemType, U: ProblemType> IntoProblem for BodyRefused<T, U> {
    fn into_problem(self) -> Problem {
        match self {
            Self::TooLarge(refusal) => refusal.into_problem(),
            Self::TooSlow(refusal) => refusal.problem(),
        }
    }

    fn statuses() -> &'static [http::StatusCode] {
        &[
            http::StatusCode::REQUEST_TIMEOUT,
            http::StatusCode::PAYLOAD_TOO_LARGE,
        ]
    }
}

impl<T: ProblemType, U: ProblemType> IntoResponse for BodyRefused<T, U> {
    fn into_response(self) -> http::Response {
        match self {
            Self::TooLarge(refusal) => refusal.into_response(),
            Self::TooSlow(refusal) => refusal.into_response(),
        }
    }
}

impl<T: ProblemType, U: ProblemType> ShortCircuit for BodyRefused<T, U> {
    const STATUSES: &'static [u16] = &[408, 413];
}

impl<T: ProblemType, U: ProblemType> Responses for BodyRefused<T, U> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let mut responses = kynos_openapi::Responses::new().with(
            408,
            refusal_response::<U>(
                registry,
                408,
                "the request body did not arrive within the configured limit",
            ),
        );
        responses.merge_from(&BodySizeExceeded::<T>::responses(registry));
        responses
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
/// for has already been spent. Where no `BodySize` covers the operation,
/// `Records` holds a chunked body to the operation's limit itself, and its 413
/// follows whatever records arrived within it, mid-stream.
///
/// # Naming what the 413 is
///
/// [`problem_type`](BodySize::problem_type) puts an application's own URI on
/// the refusal, so a client can tell an oversized upload from every other 413
/// the service sends. See [`ProblemType`] for why it is a type and not a value.
///
/// # Bounding how long the body takes
///
/// [`deadline`](BodySize::deadline) adds a clock to the cap, and a 408 to
/// what every covered operation declares: the whole body, declared-length or
/// chunked, must arrive within it. The handler beneath is not bounded, so an
/// operation whose work legitimately takes minutes still gets its body's
/// total bounded, which a [`Timeout`](super::timeout::Timeout) cannot do
/// without bounding that work too. The clock and the cap live in one
/// interceptor, so no mounting order leaves either unbounded.
///
/// The clock starts when the request reaches this interceptor. A chunked body
/// is counted under it here, and the 408 precedes the handler. A
/// declared-length body is still handed on unread: a read of it that waits
/// past the deadline fails, and once the chain beneath returns, that failure
/// is answered with the 408 in place of whatever the operation made of it.
/// Octets that have already arrived are delivered however late they are read,
/// so a reader that is late is not taken for a client that is; but a reader
/// that defers its read past the deadline leaves the client blocked behind
/// flow control, and then the wait is charged to the body. A body nothing
/// reads is never late, and a response the chain had already returned before
/// a read failed stands, since its head may be on the wire.
///
/// It complements the server's
/// [`request_body_idle_timeout`](crate::server::Server::request_body_idle_timeout)
/// rather than replacing it: that bounds the gap between frames, and a client
/// that sends one octet just inside every gap passes it and not this.
pub struct BodySize<T = (), D = ()> {
    /// The maximum body size, in bytes.
    pub limit: u64,
    /// The 408 a late body is answered with, which carries the deadline; `()`
    /// where there is none.
    deadline: D,
    /// Names the refusal's problem type without holding one.
    problem_type: PhantomData<fn() -> T>,
}

impl BodySize<()> {
    /// Caps bodies at `bytes`.
    #[must_use]
    pub fn new(bytes: u64) -> Self {
        Self {
            limit: bytes,
            deadline: (),
            problem_type: PhantomData,
        }
    }
}

impl<D> BodySize<(), D> {
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
    pub fn problem_type<T: ProblemType>(self) -> BodySize<T, D> {
        BodySize {
            limit: self.limit,
            deadline: self.deadline,
            problem_type: PhantomData,
        }
    }
}

impl<T> BodySize<T> {
    /// Bounds how long the whole body may take to arrive, declared-length or
    /// chunked, answering 408 past `total`; the handler beneath is not
    /// bounded.
    ///
    /// Changes the type, because every covered operation then declares the
    /// 408 beside the 413. Available only on a limit with no deadline, so a
    /// chain states one at most once.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use kynos::middleware::limits::body_size::BodySize;
    ///
    /// let limit = BodySize::new(1_024 * 1_024).deadline(Duration::from_secs(30));
    /// # let _ = limit;
    /// ```
    ///
    /// A [`Timeout`](super::timeout::Timeout) declares 408 too, so the two do
    /// not compile over one operation, since one status could not say which
    /// bound a client passed; give the timeout another status through
    /// [`answer_with`](super::timeout::Timeout::answer_with):
    ///
    /// ```compile_fail
    /// use std::time::Duration;
    ///
    /// use kynos::{
    ///     Router,
    ///     middleware::limits::{body_size::BodySize, timeout::Timeout},
    /// };
    ///
    /// let service = Router::<()>::new()
    ///     .intercept(Timeout::new(Duration::from_secs(300)))
    ///     .intercept(BodySize::new(1_024).deadline(Duration::from_secs(30)))
    ///     .build(());
    /// # let _ = service;
    /// ```
    #[must_use]
    pub fn deadline(self, total: Duration) -> BodySize<T, BodyTooSlow> {
        BodySize {
            limit: self.limit,
            deadline: BodyTooSlow::new(total),
            problem_type: PhantomData,
        }
    }
}

impl<T> BodySize<T, BodyTooSlow> {
    /// Names the RFC 9457 problem type this limit's 408 carries, as
    /// [`problem_type`](BodySize::problem_type) names the 413's.
    ///
    /// Available only on a deadline that has not named one, so a chain states
    /// the type at most once.
    ///
    /// ```
    /// # use std::time::Duration;
    /// use kynos::{error::problem::ProblemType, middleware::limits::body_size::BodySize};
    ///
    /// struct TooSlow;
    ///
    /// impl ProblemType for TooSlow {
    ///     const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/too-slow");
    /// }
    ///
    /// let limit = BodySize::new(1_024)
    ///     .deadline(Duration::from_secs(30))
    ///     .deadline_problem_type::<TooSlow>();
    /// # let _ = limit;
    /// ```
    #[must_use]
    pub fn deadline_problem_type<U: ProblemType>(self) -> BodySize<T, BodyTooSlow<U>> {
        BodySize {
            limit: self.limit,
            deadline: BodyTooSlow::new(self.deadline.after),
            problem_type: PhantomData,
        }
    }
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
        let body = collect_capped(body, self.limit)
            .await
            .into_body()
            .ok_or_else(|| BodySizeExceeded::new(self.limit))?;

        let request = http::Request::from_parts(parts, body);
        Ok(next.run(request).await)
    }
}

impl<C, T, U> Interceptor<C> for BodySize<T, BodyTooSlow<U>>
where
    C: Sync + 'static,
    T: ProblemType,
    U: ProblemType,
{
    type Reads = ();
    type Adds = ();
    type Short = BodyRefused<T, U>;

    async fn intercept(
        &self,
        mut request: http::Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, BodyRefused<T, U>> {
        let _ = (reads, context);

        let total = self.deadline.after;
        // `Instant + Duration` can panic; a deadline past the clock never fires.
        let deadline = Instant::now().checked_add(total);
        let version = request.version();
        let late = || BodyRefused::TooSlow(BodyTooSlow::new(total).answering(version));

        request.extensions_mut().insert(BodyLimit(self.limit));

        if let Some(declared) = declared_length(request.headers()) {
            if declared > self.limit {
                return Err(BodyRefused::TooLarge(BodySizeExceeded::new(self.limit)));
            }

            let (parts, body) = request.into_parts();
            if body.is_end_stream() {
                return Ok(next.run(http::Request::from_parts(parts, body)).await);
            }

            let overdue = Arc::new(AtomicBool::new(false));
            let body = Body::from_body(Deadlined {
                inner: body,
                deadline,
                timer: None,
                overdue: Arc::clone(&overdue),
                after: total,
                spent: false,
            });
            let continued = next.run(http::Request::from_parts(parts, body)).await;

            // Read after the chain returned, as the server's idle timer is, so
            // a response that predates the failed read stands.
            if overdue.load(Ordering::Acquire) {
                return Err(late());
            }
            return Ok(continued);
        }

        let (parts, body) = request.into_parts();
        // Nothing has awaited since `deadline` was read, so `total` from now is
        // that instant; `timeout` saturates where it would overflow.
        let capped = tokio::time::timeout(total, collect_capped(body, self.limit))
            .await
            .map_err(|_elapsed| late())?;
        let body = capped
            .into_body()
            .ok_or_else(|| BodyRefused::TooLarge(BodySizeExceeded::new(self.limit)))?;

        Ok(next.run(http::Request::from_parts(parts, body)).await)
    }
}

/// The error a declared-length body ends with once a read of it waits past its
/// deadline.
///
/// The reader sees a failed read; the interceptor then answers the 408 in place
/// of whatever the reader made of it.
#[derive(Debug)]
struct Late {
    after: Duration,
}

impl std::fmt::Display for Late {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the request body did not arrive within {:?}",
            self.after
        )
    }
}

impl std::error::Error for Late {}

/// A request body whose reads fail once one waits past its deadline.
///
/// The body is polled before the clock, so a frame that has already arrived is
/// delivered however late it is read. The timer is allocated by the first read
/// that waits, so a body that is already buffered costs none, and it is boxed so
/// this needs no projection.
struct Deadlined {
    inner: Body,
    /// When reads stop waiting; `None` where that is past the clock's range.
    deadline: Option<Instant>,
    timer: Option<Pin<Box<Sleep>>>,
    /// Set once the deadline fired, for the interceptor to read back.
    overdue: Arc<AtomicBool>,
    /// The deadline's length, which the error reports.
    after: Duration,
    /// Set once the deadline fired, so the error is yielded exactly once.
    spent: bool,
}

impl HttpBody for Deadlined {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = self.get_mut();

        if this.spent {
            return Poll::Ready(None);
        }

        if let Poll::Ready(polled) = Pin::new(&mut this.inner).poll_frame(context) {
            return Poll::Ready(polled);
        }

        let Some(deadline) = this.deadline else {
            return Poll::Pending;
        };
        let timer = this.timer.get_or_insert_with(|| {
            Box::pin(tokio::time::sleep(
                deadline.saturating_duration_since(Instant::now()),
            ))
        });
        if timer.as_mut().poll(context).is_pending() {
            return Poll::Pending;
        }

        this.spent = true;
        this.overdue.store(true, Ordering::Release);
        Poll::Ready(Some(Err(Box::new(Late { after: this.after }))))
    }

    fn is_end_stream(&self) -> bool {
        self.spent || self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

// Not derived: a derive would bound each on its markers.
impl<T, U> Clone for BodyRefused<T, U> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, U> Copy for BodyRefused<T, U> {}

impl<T, U> std::fmt::Debug for BodyRefused<T, U> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(refusal) => formatter.debug_tuple("TooLarge").field(refusal).finish(),
            Self::TooSlow(refusal) => formatter.debug_tuple("TooSlow").field(refusal).finish(),
        }
    }
}

impl<T, U> PartialEq for BodyRefused<T, U> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::TooLarge(one), Self::TooLarge(other)) => one == other,
            (Self::TooSlow(one), Self::TooSlow(other)) => one == other,
            _ => false,
        }
    }
}

impl<T, U> Eq for BodyRefused<T, U> {}

marker_traits!(BodySizeExceeded<T> { problem_type: _, limit }: Copy, Debug, PartialEq);
marker_traits!(BodyTooSlow<U> { problem_type: _, after, closes }: Copy, Debug, PartialEq);
marker_traits!(BodySize<T, D> where D { problem_type: _, limit, deadline }: Copy);

// By hand: a limit with no deadline prints none, and one with a deadline prints
// its length rather than the refusal carrying it.
impl<T> std::fmt::Debug for BodySize<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BodySize")
            .field("limit", &self.limit)
            .finish()
    }
}

impl<T, U> std::fmt::Debug for BodySize<T, BodyTooSlow<U>> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BodySize")
            .field("limit", &self.limit)
            .field("deadline", &self.deadline.after)
            .finish()
    }
}
