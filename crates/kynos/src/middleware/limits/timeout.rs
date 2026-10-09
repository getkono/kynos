//! A cap on how long a handler may run, and the 408 it answers with.

use std::{fmt, marker::PhantomData, time::Duration};

use crate::{
    error::problem::{ProblemType, refusal_problem, refusal_response},
    http,
    middleware::{Continued, Interceptor, Next},
    response::{IntoResponse, Responses, ShortCircuit},
    schema::registry::Registry,
};

/// What [`Timeout`] answers with when a handler runs too long.
///
/// `T` names the problem type the body carries; `()` leaves `about:blank`. Set
/// it with [`Timeout::problem_type`], or replace this type outright with
/// [`Timeout::answer_with`].
pub struct TimedOut<T = ()> {
    /// The limit the handler passed.
    pub after: Duration,
    /// Carries `T` without storing one, as in
    /// [`BodySizeExceeded`](super::body_size::BodySizeExceeded).
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
/// is not available: [`Concurrency`](super::concurrency::Concurrency) declares
/// it, so `CompatibleWith` would refuse a router carrying both. Bounding
/// handler time *and* capping concurrency is an ordinary pairing, and a status
/// choice that made it uncompilable would be a worse answer than an inexact
/// one.
///
/// # Mount it outside a [`BodySize`](super::body_size::BodySize)
///
/// A timeout wraps whatever is beneath it, so it bounds a body read only when
/// it is the *earlier* `intercept` call, per
/// [the module's ordering rule](crate::middleware#the-order-a-chain-runs-in).
/// `BodySize` walks a length-less body frame by frame, and a client that trickles
/// frames holds that loop open. The server's idle timer
/// (`Server::request_body_idle_timeout`) ends a body that stalls between
/// frames; a `Timeout` mounted outside bounds only the total a slow but steady
/// body may take.
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
/// answers with, `CompatibleWith` sees the same `STATUSES` the compiler
/// checks against every other interceptor in the stack.
///
/// ```no_run
/// use std::time::Duration;
/// # use kynos::{
/// #     http, middleware::limits::timeout::Timeout,
/// #     response::{IntoResponse, Responses, ShortCircuit}, schema::registry::Registry,
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
    pub limit: Duration,
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
    pub fn new(limit: Duration) -> Self {
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
    /// [`BodySize::problem_type`](super::body_size::BodySize::problem_type) for
    /// the rule and its pass control.
    ///
    /// ```
    /// # use std::time::Duration;
    /// use kynos::{error::problem::ProblemType, middleware::limits::timeout::Timeout};
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
        S: ShortCircuit + From<Duration> + Send + 'static,
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

impl<R> fmt::Debug for Timeout<R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Timeout")
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

impl<C, R> Interceptor<C> for Timeout<R>
where
    C: Sync + 'static,
    R: ShortCircuit + From<Duration> + Send + 'static,
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

// Written out rather than derived, for the reason `body_size` gives: a derive
// would bound each on the marker.

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
