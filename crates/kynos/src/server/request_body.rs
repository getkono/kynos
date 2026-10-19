//! The idle timeout the server holds a request body to, and the 408 a stalled
//! one is answered with.
//!
//! The server applies it, since only it has a peer to wait on; wrapping the body
//! before the chain sees it bounds every reader at once.

use std::{
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
    error::problem::Problem,
    http::{self, HeaderValue, StatusCode, Version, body::BoxError, header},
    response::IntoResponse,
    server::error::ServerError,
};

/// How long a request body may go without a frame unless the server is told
/// otherwise: the same 30 seconds a request head is given.
pub(crate) const DEFAULT_REQUEST_BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Refuses an idle timeout that would expire before a frame could arrive.
pub(crate) fn validate_request_body_idle_timeout(
    timeout: Option<Duration>,
) -> std::result::Result<(), ServerError> {
    if timeout.is_some_and(|timeout| timeout.is_zero()) {
        return Err(ServerError::InvalidConfiguration(
            "request_body_idle_timeout must be non-zero when enabled",
        ));
    }
    Ok(())
}

/// `body`, failing once its reader has waited `limit` for a frame, and the
/// stall that [`answer`] reads back after the chain has returned.
///
/// A body that is already over, or that no limit applies to, is handed to
/// `unbounded` unwrapped.
pub(crate) fn bounded<B>(
    body: B,
    limit: Option<Duration>,
    version: Version,
    unbounded: impl FnOnce(B) -> http::body::Body,
) -> (http::body::Body, Option<Stall>)
where
    B: HttpBody<Data = Bytes> + Send + Unpin + 'static,
    B::Error: Into<BoxError>,
{
    let Some(limit) = limit.filter(|_| !body.is_end_stream()) else {
        return (unbounded(body), None);
    };
    let stall = Stall {
        stalled: Arc::new(AtomicBool::new(false)),
        limit,
        version,
    };
    let body = http::body::Body::from_body(Idle {
        inner: body,
        timer: None,
        waiting: false,
        stall: stall.clone(),
        spent: false,
    });
    (body, Some(stall))
}

/// `response`, unless the body of the request it answers stalled.
///
/// Read after the chain returned, so a full-duplex handler's response that
/// predates the stall stands: its head may be on the wire.
pub(crate) fn answer(response: http::Response, stall: Option<Stall>) -> http::Response {
    match stall {
        Some(stall) if stall.stalled.load(Ordering::Acquire) => stall.response(),
        _ => response,
    }
}

/// What one request's bounded body shares with the connection serving it.
#[derive(Clone, Debug)]
pub(crate) struct Stall {
    /// Set by the body once its timer fired.
    stalled: Arc<AtomicBool>,
    /// The limit the body passed, which the 408 reports.
    limit: Duration,
    /// The request's protocol, which decides whether the 408 closes.
    version: Version,
}

impl Stall {
    /// The 408 a stalled request is answered with.
    ///
    /// RFC 9110 §15.5.9: the server "SHOULD send the `close` connection
    /// option", so HTTP/1 closes; HTTP/2 has no such field, and the stall
    /// held only one stream.
    fn response(&self) -> http::Response {
        let mut response = Problem::new(StatusCode::REQUEST_TIMEOUT)
            .with_detail(format!(
                "the request body sent nothing for {:?}",
                self.limit
            ))
            .into_response();
        if self.version < Version::HTTP_2 {
            response
                .headers_mut()
                .insert(header::CONNECTION, HeaderValue::from_static("close"));
        }
        response
    }
}

/// The error a stalled body ends with.
///
/// The reader sees a failed read; [`answer`] then replaces its refusal with
/// the 408, since the chain cannot tell a stall from a reset.
#[derive(Debug)]
struct Stalled {
    after: Duration,
}

impl std::fmt::Display for Stalled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "no request body frame arrived within {:?}",
            self.after
        )
    }
}

impl std::error::Error for Stalled {}

/// A request body that fails once its reader has waited its limit for a frame.
///
/// The clock runs only while a reader is waiting, so a late reader or a client
/// awaiting `100 Continue` (sent on first poll) is not a stall. The timer is
/// boxed so this needs no projection.
struct Idle<B> {
    inner: B,
    /// Allocated by the first wait and reset by every later one.
    timer: Option<Pin<Box<Sleep>>>,
    /// Whether the timer is measuring the current wait.
    waiting: bool,
    stall: Stall,
    /// Set once the timer fired, so the error is yielded exactly once.
    spent: bool,
}

impl<B> HttpBody for Idle<B>
where
    B: HttpBody<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
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

        // The body first: a ready frame ends the wait however late it was asked for.
        if let Poll::Ready(polled) = Pin::new(&mut this.inner).poll_frame(context) {
            this.waiting = false;
            return Poll::Ready(polled.map(|frame| frame.map_err(Into::into)));
        }

        let limit = this.stall.limit;
        if !this.waiting {
            this.waiting = true;
            // `Instant + Duration` can panic; a limit past the clock never fires.
            let Some(deadline) = Instant::now().checked_add(limit) else {
                return Poll::Pending;
            };
            match &mut this.timer {
                Some(timer) => timer.as_mut().reset(deadline),
                None => this.timer = Some(Box::pin(tokio::time::sleep_until(deadline))),
            }
        }

        let fired = this
            .timer
            .as_mut()
            .is_some_and(|timer| timer.as_mut().poll(context).is_ready());
        if !fired {
            return Poll::Pending;
        }

        this.spent = true;
        this.stall.stalled.store(true, Ordering::Release);
        Poll::Ready(Some(Err(Box::new(Stalled { after: limit }))))
    }

    fn is_end_stream(&self) -> bool {
        self.spent || self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
