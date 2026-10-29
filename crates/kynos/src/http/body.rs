//! The request and response body.
//!
//! Opaque by design: a body is consumed through a typed extractor.

use std::{
    error::Error as StdError,
    fmt,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::{BodyExt, Empty, Full, combinators::UnsyncBoxBody};

pub(crate) type BoxError = Box<dyn StdError + Send + Sync>;

/// The body of a request or a response.
///
/// Opaque by design. A request's is consumed through a typed extractor such as
/// [`Json`](crate::extract::body::json::Json), never read directly.
pub struct Body {
    inner: Mutex<UnsyncBoxBody<Bytes, BoxError>>,
}

impl Body {
    /// An empty body.
    #[must_use]
    pub fn empty() -> Self {
        Self::from_body(Empty::new().map_err(|never| match never {}))
    }

    /// A body holding exactly these bytes.
    #[must_use]
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self::from_body(Full::new(bytes).map_err(|never| match never {}))
    }

    /// The exact length the body states, read without locking it.
    ///
    /// For an owned body, in place of [`HttpBody::size_hint`]: that takes
    /// `&self`, so it locks, and where `std`'s mutex is a lazily boxed pthread
    /// mutex (macOS among others) the first lock is a heap allocation.
    pub(crate) fn exact_length(&mut self) -> Option<u64> {
        self.inner
            .get_mut()
            .expect("a mutably borrowed body mutex cannot be poisoned")
            .size_hint()
            .exact()
    }

    /// A body that yields `arrived` and then fails with `error`.
    ///
    /// For a buffering interceptor whose read failed part-way, so the extractor
    /// beneath sees the same failure rather than a truncated whole body.
    pub(crate) fn failed_after(arrived: Bytes, error: BoxError) -> Self {
        Self::from_body(FailedAfter {
            // No empty data frame: nothing arrived, so nothing is replayed.
            arrived: (!arrived.is_empty()).then_some(arrived),
            error: Some(error),
        })
    }

    /// A body whose bytes arrive as a stream.
    pub(crate) fn from_stream<S, E>(stream: S) -> Self
    where
        S: futures_core::Stream<Item = Result<Bytes, E>> + Send + 'static,
        E: Into<BoxError> + 'static,
    {
        Self::from_body(Streamed {
            chunks: Box::pin(stream),
        })
    }

    /// A body whose bytes arrive as `chunks`, each one frame, for a fuzz
    /// target choosing where a transport splits a body.
    #[cfg(fuzzing)]
    pub(crate) fn from_chunks(chunks: Vec<Bytes>) -> Self {
        Self::from_body(Chunks(chunks.into()))
    }

    /// Erases a wrapping body (compressing, counting, ranged). Ungated, since
    /// `response::range` is.
    pub(crate) fn from_body<B>(body: B) -> Self
    where
        B: HttpBody<Data = Bytes, Error = BoxError> + Send + 'static,
    {
        Self {
            inner: Mutex::new(body.boxed_unsync()),
        }
    }

    #[cfg(feature = "server")]
    pub(crate) fn from_incoming(body: hyper::body::Incoming) -> Self {
        Self::from_body(body.map_err(Into::into))
    }

    /// Reports how this body ends, exactly once.
    ///
    /// From whichever comes first: the poll that exhausts the body, or its drop.
    ///
    /// One allocation: the box that erases the watching body, which holds
    /// `report` inline.
    pub(crate) fn watching<F>(self, report: F) -> Self
    where
        F: FnOnce(Delivery) + Send + 'static,
    {
        let inner = self
            .inner
            .into_inner()
            .expect("an owned body mutex cannot be poisoned");

        Self::from_body(watched::Watched {
            inner,
            report: Some(report),
        })
    }
}

/// A response with `status` and no content.
#[must_use]
pub(crate) fn bodiless(status: super::StatusCode) -> super::Response {
    let mut response = super::Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

/// How a body ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// The body was read to its end.
    Complete,
    /// The body was dropped before its end.
    ///
    /// A peer that went away mid-response, or a stream that failed part-way.
    Interrupted,
}

/// A stream of chunks, seen as a body: one data frame per chunk.
///
/// Boxed so it is `Unpin` and needs no projection.
struct Streamed<S> {
    chunks: Pin<Box<S>>,
}

impl<S, E> HttpBody for Streamed<S>
where
    S: futures_core::Stream<Item = Result<Bytes, E>>,
    E: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.get_mut().chunks.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(chunk))) => Poll::Ready(Some(Ok(Frame::data(chunk)))),
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error.into()))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Chunks already in memory, seen as a body: one data frame per chunk, in
/// order, as a transport might have split them.
#[cfg(fuzzing)]
struct Chunks(std::collections::VecDeque<Bytes>);

#[cfg(fuzzing)]
impl HttpBody for Chunks {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(
            self.get_mut()
                .0
                .pop_front()
                .map(|chunk| Ok(Frame::data(chunk))),
        )
    }
}

/// The bytes that arrived before a read failed, followed by the failure.
///
/// Both fields are [`Unpin`], so `poll_frame` reaches them through
/// [`Pin::get_mut`].
struct FailedAfter {
    /// `None` once replayed, or when nothing arrived.
    arrived: Option<Bytes>,
    /// `None` once reported.
    error: Option<BoxError>,
}

impl HttpBody for FailedAfter {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if let Some(arrived) = this.arrived.take() {
            return Poll::Ready(Some(Ok(Frame::data(arrived))));
        }
        Poll::Ready(this.error.take().map(Err))
    }

    // Never, or a `Watched` would report the failed body as complete.
    fn is_end_stream(&self) -> bool {
        false
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Body").finish_non_exhaustive()
    }
}

impl HttpBody for Body {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        // `UnsyncBoxBody` is already pinned internally and moving its pointer
        // does not move the erased body.
        let inner = self
            .get_mut()
            .inner
            .get_mut()
            .expect("a mutably borrowed body mutex cannot be poisoned");
        Pin::new(inner).poll_frame(context)
    }

    fn is_end_stream(&self) -> bool {
        self.inner
            .lock()
            .expect("body mutex poisoned")
            .is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.lock().expect("body mutex poisoned").size_hint()
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::empty()
    }
}

/// The same body [`Body::from_bytes`] builds.
impl From<Bytes> for Body {
    fn from(bytes: Bytes) -> Self {
        Self::from_bytes(bytes)
    }
}

/// Takes the buffer over without copying it.
impl From<Vec<u8>> for Body {
    fn from(bytes: Vec<u8>) -> Self {
        Self::from_bytes(Bytes::from(bytes))
    }
}

/// Takes the buffer over without copying it.
impl From<String> for Body {
    fn from(text: String) -> Self {
        Self::from_bytes(Bytes::from(text))
    }
}

/// Borrows the text for good rather than copying it.
impl From<&'static str> for Body {
    fn from(text: &'static str) -> Self {
        Self::from_bytes(Bytes::from_static(text.as_bytes()))
    }
}

mod watched;

#[cfg(test)]
mod tests;
