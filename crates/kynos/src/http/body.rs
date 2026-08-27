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

    /// A body that is another body, already erased.
    ///
    /// The one constructor an adapter needs: a body that wraps another -- a
    /// compressing one, a counting one, a ranged one reading spans -- is still
    /// a body, and this is how it becomes the erased kind without going through
    /// bytes or a stream.
    ///
    /// Ungated: `response::range` is behind no feature and produces one, so a
    /// `compression` gate here would make ranged delivery depend on an
    /// unrelated flag.
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
        Self {
            inner: Mutex::new(body.map_err(Into::into).boxed_unsync()),
        }
    }

    /// Reports how this body ends, exactly once.
    ///
    /// The report runs from whichever of the two ends comes first: the poll
    /// that exhausts the body, or its drop. Exactly once and never zero times
    /// is the whole property -- it is what makes the report usable as "did the
    /// peer receive this", where a signal that can be missed is worse than none.
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
                Watched {
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
    /// Ordinarily a peer that went away mid-response. A stream that failed
    /// part-way ends the same way and cannot be told apart from here, which is
    /// the honest reading anyway: in both cases what was announced was not
    /// delivered.
    Interrupted,
}

/// A body that reports how it ended.
///
/// Not generic over the callback. A boxed `FnOnce` is unconditionally [`Unpin`]
/// whatever it captures, which is what lets `poll_frame` reach its fields
/// through [`Pin::get_mut`] -- `unsafe` is forbidden here, so a hand-written
/// projection is not available.
struct Watched {
    inner: UnsyncBoxBody<Bytes, BoxError>,
    /// `None` once the report has been made, which is what makes it once.
    report: Option<Box<dyn FnOnce(Delivery) + Send>>,
}

impl Watched {
    /// Reports `delivery`, unless something already reported.
    fn report(&mut self, delivery: Delivery) {
        if let Some(report) = self.report.take() {
            report(delivery);
        }
    }
}

impl HttpBody for Watched {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.inner).poll_frame(context);

        match &polled {
            // Nothing left: everything the body had was yielded.
            Poll::Ready(None) => this.report(Delivery::Complete),
            // A body may declare its end on the frame that carries the last of
            // it rather than on a further poll, and a driver that reads the
            // declaration stops polling. Ask, so that ending is not read as an
            // interruption.
            Poll::Ready(Some(Ok(_))) if this.inner.is_end_stream() => {
                this.report(Delivery::Complete);
            }
            // A frame that failed leaves the body unfinished, and the drop
            // below reports it as such. A frame with more behind it is not an
            // ending at all.
            Poll::Ready(Some(_)) | Poll::Pending => {}
        }

        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        // A body already at its end was delivered whether or not anything
        // polled it again: an empty response is the ordinary case, and a driver
        // that consults `is_end_stream` first need never call `poll_frame` at
        // all.
        let delivery = if self.inner.is_end_stream() {
            Delivery::Complete
        } else {
            Delivery::Interrupted
        };

        self.report(delivery);
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

    use std::sync::{Arc, Mutex};

    use super::{Body, Bytes, Delivery, HttpBody};

    /// A recorder for the one report a watched body makes.
    ///
    /// A `Mutex<Vec<_>>` rather than a single slot, so that a second report --
    /// the failure the once-only rule exists to stop -- is visible as a second
    /// entry rather than overwriting the first.
    #[derive(Clone, Default)]
    struct Reports(Arc<Mutex<Vec<Delivery>>>);

    impl Reports {
        /// Watches `body`, recording how it ends.
        fn watching(&self, body: Body) -> Body {
            let reports = Arc::clone(&self.0);
            body.watching(move |delivery| {
                reports
                    .lock()
                    .expect("an unpoisoned recorder")
                    .push(delivery);
            })
        }

        /// What was reported, in order.
        fn taken(&self) -> Vec<Delivery> {
            self.0.lock().expect("an unpoisoned recorder").clone()
        }
    }

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

    /// Watching must not change what a body is: a wrapper that lost the exact
    /// length would silently switch every watched response to chunked framing.
    #[test]
    fn a_watched_body_states_the_length_the_body_beneath_it_states() {
        let reports = Reports::default();
        let body = reports.watching(Body::from_bytes(Bytes::from_static(b"1234")));

        assert_eq!(body.size_hint().exact(), Some(4));
        assert!(!body.is_end_stream());
    }

    #[tokio::test]
    async fn a_watched_body_read_to_its_end_reports_delivery_once() {
        let reports = Reports::default();
        let body = reports.watching(Body::from_bytes(Bytes::from_static(b"1234")));

        assert_eq!(drain(body).await, Bytes::from_static(b"1234"));
        assert_eq!(reports.taken(), vec![Delivery::Complete]);
    }

    /// The signal this exists for. The bytes were there and nothing read them,
    /// which from the peer's side is a response that never arrived.
    #[tokio::test]
    async fn a_watched_body_dropped_before_its_end_reports_an_interruption() {
        let reports = Reports::default();

        drop(reports.watching(Body::from_bytes(Bytes::from_static(b"1234"))));

        assert_eq!(reports.taken(), vec![Delivery::Interrupted]);
    }

    /// The pass control for the case above, differing in exactly one property:
    /// there was nothing to deliver. An empty response is the ordinary one, and
    /// reporting every 204 as an interruption would make the signal useless.
    #[tokio::test]
    async fn a_watched_empty_body_reports_delivery_even_unpolled() {
        let reports = Reports::default();

        drop(reports.watching(Body::empty()));

        assert_eq!(reports.taken(), vec![Delivery::Complete]);
    }

    /// Once, not twice. The drop runs after the read that already reported, and
    /// a duplicate would double-count every completed response.
    #[tokio::test]
    async fn a_watched_body_reports_once_across_both_of_its_ends() {
        let reports = Reports::default();
        let body = reports.watching(Body::from_bytes(Bytes::from_static(b"1234")));

        let _ = drain(body).await;

        assert_eq!(
            reports.taken(),
            vec![Delivery::Complete],
            "the drop reported a second time over the read that had already reported"
        );
    }
}
