//! A cap on concurrent in-flight requests, and the 503 it answers with.

use std::{fmt, marker::PhantomData, num::NonZeroUsize, sync::Arc, time::Duration};

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
    /// Carries `T` without storing one, as in
    /// [`BodySizeExceeded`](super::body_size::BodySizeExceeded).
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
/// # use kynos::middleware::limits::concurrency::Concurrency;
/// let concurrency = Concurrency::new(NonZeroUsize::new(64).expect("nonzero"));
/// assert_eq!(concurrency.limit.get(), 64);
/// ```
///
/// Zero has no `NonZeroUsize` to be, so the mistake does not compile:
///
/// ```compile_fail
/// # use kynos::middleware::limits::concurrency::Concurrency;
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
    /// turbofish, as [`BodySize::new`](super::body_size::BodySize::new) is.
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
    /// type at most once. See
    /// [`BodySize::problem_type`](super::body_size::BodySize::problem_type) for
    /// the rule and its pass control.
    ///
    /// ```
    /// # use std::num::NonZeroUsize;
    /// use kynos::{error::problem::ProblemType, middleware::limits::concurrency::Concurrency};
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

// Written out rather than derived, for the reason `body_size` gives: a derive
// would bound each on the marker.

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
