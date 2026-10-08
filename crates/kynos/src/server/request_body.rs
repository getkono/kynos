//! The idle timeout on a request body, and the 408 a stalled one answers with.
//!
//! The timer lives here rather than in an extractor because only the server
//! has a peer to wait on: a body a test or an embedding builds in memory never
//! stalls, and every reader of a socket-backed body -- each codec, `BodySize`'s
//! count, a decompressor, a streamed `Records` -- is bounded by wrapping the
//! body once, before any of them sees it.

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
pub(in crate::server) const DEFAULT_REQUEST_BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Refuses an idle timeout that would expire before a frame could arrive.
pub(in crate::server) fn validate_request_body_idle_timeout(
    timeout: Option<Duration>,
) -> std::result::Result<(), ServerError> {
    if timeout.is_some_and(|timeout| timeout.is_zero()) {
        return Err(ServerError::InvalidConfiguration(
            "request_body_idle_timeout must be non-zero when enabled",
        ));
    }
    Ok(())
}

/// Set once a request's body stalled, so the connection can answer 408 in
/// place of whatever the chain made of the failed read.
#[derive(Clone, Debug)]
pub(in crate::server) struct Stalled {
    flag: Arc<AtomicBool>,
    /// The limit the body passed, which the 408 reports.
    limit: Duration,
}

impl Stalled {
    fn new(limit: Duration) -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            limit,
        }
    }

    /// Whether the body this flag watches went its limit without a frame.
    pub(in crate::server) fn is_set(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    fn set(&self) {
        self.flag.store(true, Ordering::Release);
    }

    /// What the request is answered with once its body stalled.
    ///
    /// RFC 9110 §15.5.9: the server "did not receive a complete request
    /// message within the time that it was prepared to wait", and it "SHOULD
    /// send the `close` connection option" -- so an HTTP/1 connection is
    /// closed rather than left to frame whatever the client sends next as a
    /// new request. HTTP/2 carries no connection-specific field, and needs
    /// none: the stall held one stream, and answering it ends that stream
    /// alone.
    pub(in crate::server) fn response(&self, version: Version) -> http::Response {
        let mut response = Problem::new(StatusCode::REQUEST_TIMEOUT)
            .with_detail(format!(
                "the request body sent nothing for {:?}",
                self.limit
            ))
            .into_response();
        if version < Version::HTTP_2 {
            response
                .headers_mut()
                .insert(header::CONNECTION, HeaderValue::from_static("close"));
        }
        response
    }
}

/// The request body hyper delivered, bounded by `limit` between frames, and
/// the flag its stall sets.
///
/// A body that is already over -- every bodyless `GET` -- is handed on as it
/// is and watched by nothing: there is no frame left to wait for, and the
/// wrapper would cost the request an allocation for a timer that cannot fire.
pub(in crate::server) fn bounded(
    body: hyper::body::Incoming,
    limit: Option<Duration>,
) -> (http::body::Body, Option<Stalled>) {
    match limit {
        Some(limit) if !body.is_end_stream() => {
            let (body, stalled) = idle(body, limit);
            (body, Some(stalled))
        }
        _ => (http::body::Body::from_incoming(body), None),
    }
}

/// `body`, failing once its reader has waited `limit` for a frame.
fn idle<B>(body: B, limit: Duration) -> (http::body::Body, Stalled)
where
    B: HttpBody<Data = Bytes> + Send + Unpin + 'static,
    B::Error: Into<BoxError>,
{
    let stalled = Stalled::new(limit);
    let body = http::body::Body::from_body(Idle {
        inner: body,
        limit,
        timer: None,
        waiting: false,
        stalled: stalled.clone(),
        spent: false,
    });
    (body, stalled)
}

/// The error a stalled body ends with.
///
/// What the reader sees is a failed read, so each extractor refuses it the way
/// it refuses any transport failure; the connection then replaces that refusal
/// with the 408, since the chain cannot tell a stall from a reset.
#[derive(Debug)]
struct StalledBody {
    after: Duration,
}

impl std::fmt::Display for StalledBody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "no request body frame arrived within {:?}",
            self.after
        )
    }
}

impl std::error::Error for StalledBody {}

/// A request body that fails once its reader has waited `limit` for a frame.
///
/// The clock runs only while a reader is waiting: it is armed by the poll that
/// finds nothing, and disarmed by the frame that ends the wait. A handler that
/// reads its body late, or a client waiting on `100 Continue` -- which hyper
/// sends only once the body is first polled -- is therefore not counted as a
/// stall.
///
/// The timer is boxed so this needs no projection, which is the same reason
/// `BodyTimeout` boxes its own.
struct Idle<B> {
    inner: B,
    limit: Duration,
    /// Allocated by the first wait and reset by every later one.
    timer: Option<Pin<Box<Sleep>>>,
    /// Whether the timer is measuring the current wait.
    waiting: bool,
    stalled: Stalled,
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

        // The body first, and the clock only when it has nothing: a frame that
        // is ready ends the wait however long the reader took to ask for it.
        if let Poll::Ready(polled) = Pin::new(&mut this.inner).poll_frame(context) {
            this.waiting = false;
            return Poll::Ready(polled.map(|frame| frame.map_err(Into::into)));
        }

        if !this.waiting {
            this.waiting = true;
            // `checked_add` because `Instant + Duration` panics where the limit
            // is the operator's number; a limit past the clock never fires.
            let Some(deadline) = Instant::now().checked_add(this.limit) else {
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
        this.stalled.set();
        Poll::Ready(Some(Err(Box::new(StalledBody { after: this.limit }))))
    }

    fn is_end_stream(&self) -> bool {
        self.spent || self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
