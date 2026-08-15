//! The request and response body.
//!
//! Opaque by design, and the one place the erased body type is named. A body is
//! consumed through a typed extractor, so nothing above this file needs to know
//! what it is erased into.

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

type BoxError = Box<dyn StdError + Send + Sync>;

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

    /// A body whose bytes arrive as a stream.
    ///
    /// The one place a stream becomes a body, which is what keeps
    /// [`response::stream`](crate::response::stream) clear of the body trait:
    /// each module there frames its items into `Bytes` and hands them over.
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

    #[cfg(feature = "server")]
    pub(crate) fn from_incoming(body: hyper::body::Incoming) -> Self {
        Self {
            inner: Mutex::new(body.map_err(Into::into).boxed_unsync()),
        }
    }
}

/// A stream of chunks, seen as a body: one data frame per chunk.
///
/// Boxed so that it can be polled without a projection: `Pin<Box<S>>` is
/// `Unpin` whatever `S` is, and `unsafe` is forbidden here.
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

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;

    use super::{Body, Bytes, HttpBody};

    /// Reads a body to the bytes it carries, driving `poll_frame` to its end.
    async fn drain(body: Body) -> Bytes {
        body.collect()
            .await
            .expect("a body built from bytes cannot fail")
            .to_bytes()
    }

    #[tokio::test]
    async fn an_empty_body_carries_no_bytes() {
        assert!(drain(Body::empty()).await.is_empty());
    }

    #[tokio::test]
    async fn a_body_carries_exactly_the_bytes_it_was_given() {
        let bytes = Bytes::from_static(br#"{"id":1}"#);
        assert_eq!(drain(Body::from_bytes(bytes.clone())).await, bytes);
    }

    #[tokio::test]
    async fn the_default_body_is_the_empty_one() {
        assert!(drain(Body::default()).await.is_empty());
    }

    /// Both answers come from a lock rather than from the erased body directly,
    /// so they are worth asking before anything has read a frame -- which is
    /// when a `Content-Length` is decided.
    #[test]
    fn an_empty_body_states_its_end_and_its_length() {
        let body = Body::empty();

        assert!(body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(0));
    }

    #[test]
    fn a_body_states_its_length_before_it_is_read() {
        let body = Body::from_bytes(Bytes::from_static(b"1234"));

        assert!(!body.is_end_stream());
        assert_eq!(body.size_hint().exact(), Some(4));
    }
}
