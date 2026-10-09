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

/// The request body.
///
/// Opaque by design. Bodies are consumed through a typed extractor such as
/// [`Json`](crate::extract::body::json::Json), never read directly.
pub struct Body {
    inner: Mutex<UnsyncBoxBody<Bytes, BoxError>>,
}

impl Body {
    /// An empty body.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            inner: Mutex::new(Empty::new().map_err(|never| match never {}).boxed_unsync()),
        }
    }

    /// A body holding exactly these bytes.
    #[must_use]
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self {
            inner: Mutex::new(
                Full::new(bytes)
                    .map_err(|never| match never {})
                    .boxed_unsync(),
            ),
        }
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
    #[cfg(feature = "openapi32")]
    pub(crate) fn from_stream<S, E>(stream: S) -> Self
    where
        S: futures_core::Stream<Item = Result<Bytes, E>> + Send + 'static,
        E: Into<BoxError> + 'static,
    {
        Self {
            inner: Mutex::new(
                Streamed {
                    chunks: Box::pin(stream),
                }
                .boxed_unsync(),
            ),
        }
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

    /// A body whose read fails with `error`, having yielded nothing.
    ///
    /// For a buffering interceptor whose read failed, so the connection driver
    /// aborts the message rather than framing a short one as complete.
    #[cfg(any(feature = "cache", feature = "compression"))]
    pub(crate) fn failed(error: BoxError) -> Self {
        Self {
            inner: Mutex::new(Failed { error: Some(error) }.boxed_unsync()),
        }
    }

    #[cfg(feature = "server")]
    pub(crate) fn from_incoming(body: hyper::body::Incoming) -> Self {
        Self {
            inner: Mutex::new(body.map_err(Into::into).boxed_unsync()),
        }
    }

    /// Reports how this body ends, exactly once.
    ///
    /// From whichever comes first: the poll that exhausts the body, or its drop.
    pub(crate) fn watching<F>(self, report: F) -> Self
    where
        F: FnOnce(Delivery) + Send + 'static,
    {
        let inner = self
            .inner
            .into_inner()
            .expect("an owned body mutex cannot be poisoned");

        Self {
            inner: Mutex::new(
                watched::Watched {
                    inner,
                    report: Some(Box::new(report)),
                }
                .boxed_unsync(),
            ),
        }
    }
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

/// A body that fails once and yields nothing else.
///
/// Never reports its end, or [`Watched`](watched::Watched) would read it as
/// complete.
#[cfg(any(feature = "cache", feature = "compression"))]
struct Failed {
    error: Option<BoxError>,
}

#[cfg(any(feature = "cache", feature = "compression"))]
impl HttpBody for Failed {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(self.get_mut().error.take().map(Err))
    }
}

/// A stream of chunks, seen as a body: one data frame per chunk.
///
/// Boxed so it is `Unpin` and needs no projection.
#[cfg(feature = "openapi32")]
struct Streamed<S> {
    chunks: Pin<Box<S>>,
}

#[cfg(feature = "openapi32")]
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
