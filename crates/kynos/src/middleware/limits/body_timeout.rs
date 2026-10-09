//! A cap on how long a response body may take.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;

use crate::{
    http::{self, body::BoxError},
    middleware::{Continued, Interceptor, Next},
};

/// The error a body bounded by [`BodyTimeout`] ends with.
///
/// Reaches a client as a truncated response, since the head has already left.
/// It is an error rather than a clean end so that the protocol driver resets
/// the stream instead of framing the truncation as a complete body.
///
/// A caller reads it back out of [`Body::Error`](http_body::Body::Error) by
/// downcasting the boxed error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyTimedOut {
    after: Duration,
}

impl BodyTimedOut {
    /// The limit the body passed.
    #[must_use]
    pub const fn after(&self) -> Duration {
        self.after
    }
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
/// and the headers have already left. [`Timeout`](super::timeout::Timeout)
/// stops once the head is ready, so a streamed body (Server-Sent Events, JSON
/// Lines) is bounded only by this.
///
/// # Idle, or a deadline
///
/// [`idle`](BodyTimeout::idle) restarts the clock on every frame, so it bounds
/// the *gap* between frames. [`deadline`](BodyTimeout::deadline) never restarts
/// it, so it bounds the total time a body may take.
///
/// Idle is the default choice: a deadline ends a healthy long-lived stream, and
/// suits a body of bounded size.
///
/// # Where it sits around a buffering interceptor
///
/// The clock starts when the chain beneath has returned a head, and bounds
/// only the body it is then handed. An interceptor that *buffers* --
/// [`Compression`](crate::middleware::compression::Compression) over a body of
/// known length, a cache storing a response -- reads that body to the end
/// inside the chain, so the placement decides what is covered:
///
/// - Outside the interceptor, this bounds a body it streams through or
///   declines unread, but not a buffered read: that read runs before the clock
///   starts, and what it wraps afterwards is already in memory.
/// - Beneath it, this bounds the buffered read. The interceptor hands on a body
///   that fails the same way and stores nothing, so the driver resets the
///   stream, though none of the octets read before the failure reach the
///   client.
/// - `Timeout` outside the interceptor bounds the chain's future, which is
///   where the buffered read runs, so it bounds that read too.
///
/// Nothing checks the placement.
///
/// # Server-Sent Events reset an idle timer
///
/// A keep-alive is a real frame, so an event stream whose keep-alive interval
/// is shorter than `limit` never trips `idle`. Use `deadline` to bound how long
/// such a stream may run at all.
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
    // No status: the head is already gone when this fires.
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
/// The timer is boxed so this is [`Unpin`] and needs no projection.
struct Bounded {
    inner: http::body::Body,
    timer: Pin<Box<tokio::time::Sleep>>,
    limit: Duration,
    /// Whether a frame restarts the clock.
    reset_each_frame: bool,
    /// Set once the timer has fired, so the error is yielded exactly once.
    spent: bool,
}

impl http_body::Body for Bounded {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();

        if this.spent {
            return Poll::Ready(None);
        }

        // A body that has already ended was delivered, whatever the clock says.
        if this.inner.is_end_stream() {
            return Poll::Ready(None);
        }

        if this.reset_each_frame {
            // The inner body first: an idle limit bounds the producer, and a
            // slow reader stretches the gap between polls without the producer
            // stalling.
            let polled = Pin::new(&mut this.inner).poll_frame(context);

            if matches!(polled, Poll::Ready(Some(Ok(_)))) {
                // `Instant + Duration` panics where `sleep` saturates.
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

        // A deadline goes first: a steadily producing body is what it ends.
        if this.timer.as_mut().poll(context).is_ready() {
            this.spent = true;
            return Poll::Ready(Some(Err(Box::new(BodyTimedOut { after: this.limit }))));
        }

        Pin::new(&mut this.inner).poll_frame(context)
    }

    // Not `self.spent || ..`: `Watched` reads this to tell `Complete` from
    // `Interrupted`, and a timed-out body did not complete.
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        // The inner hint stands, so a truncation is never framed as complete.
        self.inner.size_hint()
    }
}
