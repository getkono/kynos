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
/// Reaches a client as a truncated response and nothing else: the status and
/// the headers left before the timer did, so there is no status left to send.
/// It is an error rather than a clean end so that the protocol driver resets
/// the stream instead of framing the truncation as a complete body — a
/// consumer that reads a length or a terminating chunk has to be able to tell
/// the two apart.
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
/// and the headers have already left. What this bounds is the part of a
/// response [`Timeout`](super::timeout::Timeout) cannot see.
///
/// # Why `Timeout` does not already cover this
///
/// `Timeout` wraps the chain's future, and that future completes when the
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
/// Nothing checks the placement: `CompatibleWith` compares sets, and a set has
/// no positions.
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
    timer: Pin<Box<tokio::time::Sleep>>,
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
            let polled = Pin::new(&mut this.inner).poll_frame(context);

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

        Pin::new(&mut this.inner).poll_frame(context)
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
