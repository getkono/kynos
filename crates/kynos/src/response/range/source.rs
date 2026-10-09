//! Where ranged octets come from.
//!
//! [`Rangeable`](super::rangeable) is the sealed set of bodies a range can be
//! *sliced* from, octets already in hand. [`ByteSource`] is the open set it can
//! be *read* from: an object store, a fake in a test, a file. Reading is
//! asynchronous and partial, so the complete representation never has to be in
//! memory at once.
//!
//! A source is asked for a span and returns that span, so serving a kilobyte
//! out of a gigabyte reads a kilobyte. A short answer is asked again for the
//! remainder; an empty one fails the body with [`Truncated`].
//!
//! Nothing here names tokio; a source that reads a file names it in the
//! application's own implementation.

use std::{future::Future, pin::Pin, task::Poll};

use bytes::Bytes;

/// How much of a representation is read at a time.
///
/// A response is streamed in spans of this size, so the full representation is
/// never buffered and a slow client holds one span rather than one file.
pub const SPAN: u64 = 64 * 1024;

/// Octets a byte range can be read from.
///
/// Implement this over whatever holds the representation.
///
/// ```
/// use bytes::Bytes;
/// use kynos::response::range::source::ByteSource;
///
/// /// A representation held in memory, which is what a test fake usually is.
/// struct InMemory(Bytes);
///
/// impl ByteSource for InMemory {
///     type Error = std::convert::Infallible;
///
///     async fn complete_length(&self) -> Result<u64, Self::Error> {
///         Ok(self.0.len() as u64)
///     }
///
///     async fn read_span(&self, first: u64, last: u64) -> Result<Bytes, Self::Error> {
///         let first = usize::try_from(first).unwrap_or(usize::MAX).min(self.0.len());
///         let end = usize::try_from(last.saturating_add(1))
///             .unwrap_or(usize::MAX)
///             .min(self.0.len());
///         Ok(self.0.slice(first..end))
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a byte source",
    label = "cannot be read as ranged octets",
    note = "implement `ByteSource` for it: a `complete_length` and a `read_span`, both async",
    note = "for octets already in hand, `Binary<M>` is a `Rangeable` and needs no source"
)]
pub trait ByteSource: Send + Sync + 'static {
    /// What went wrong reading.
    ///
    /// [`Served`](super::served::Served) hands it back, so the application
    /// decides whether it is a 404 or a 500.
    type Error: std::error::Error + Send + Sync + 'static;

    /// How many octets the whole representation has.
    ///
    /// Asked once per request, before anything is read, so an unsatisfiable
    /// request costs no read (RFC 9110 sections 14.1.2, 14.4).
    fn complete_length(&self) -> impl Future<Output = Result<u64, Self::Error>> + Send;

    /// The octets from `first` to `last`, inclusive.
    ///
    /// Both offsets are within the length this source last reported, so an
    /// implementation does not have to bounds-check them against it.
    ///
    /// Returning fewer octets than asked for is a short read: the remainder is
    /// asked for on the next poll. Returning *none* fails the body with
    /// [`Truncated`].
    fn read_span(
        &self,
        first: u64,
        last: u64,
    ) -> impl Future<Output = Result<Bytes, Self::Error>> + Send;
}

/// A source stopped short of the length it reported.
///
/// Raised when [`ByteSource::read_span`] answers a non-empty span with no
/// octets, as when a file shrinks after
/// [`complete_length`](ByteSource::complete_length) was read.
///
/// It arrives on the body stream rather than as a status, because
/// [`Served::deliver`](super::served::Served::deliver) has already sent a
/// `Content-Length` sized from the complete length; failing the body tells the
/// recipient it did not get what was promised. Read it back by downcasting
/// [`Body::Error`](http_body::Body::Error).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Truncated {
    first: u64,
    last: u64,
}

impl Truncated {
    /// The first offset the source was asked for.
    #[must_use]
    pub const fn first(&self) -> u64 {
        self.first
    }

    /// The last offset the source was asked for, inclusive.
    #[must_use]
    pub const fn last(&self) -> u64 {
        self.last
    }
}

impl std::fmt::Display for Truncated {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "byte source returned no octets for {}..={}, so the representation is shorter than the complete length it reported",
            self.first, self.last
        )
    }
}

impl std::error::Error for Truncated {}

/// A representation held in memory.
///
/// The degenerate source, and the one a test reaches for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InMemory(Bytes);

impl InMemory {
    /// A source over octets already held.
    #[must_use]
    pub const fn new(octets: Bytes) -> Self {
        Self(octets)
    }
}

impl From<Bytes> for InMemory {
    fn from(octets: Bytes) -> Self {
        Self(octets)
    }
}

impl ByteSource for InMemory {
    type Error = std::convert::Infallible;

    async fn complete_length(&self) -> Result<u64, Self::Error> {
        Ok(u64::try_from(self.0.len()).unwrap_or(u64::MAX))
    }

    async fn read_span(&self, first: u64, last: u64) -> Result<Bytes, Self::Error> {
        Ok(clamped(&self.0, first, last))
    }
}

/// The octets from `first` to `last` inclusive, clamped to what is there.
fn clamped(octets: &Bytes, first: u64, last: u64) -> Bytes {
    let len = octets.len();
    let first = usize::try_from(first).unwrap_or(usize::MAX).min(len);
    let end = usize::try_from(last.saturating_add(1))
        .unwrap_or(usize::MAX)
        .min(len);
    octets.slice(first..end.max(first))
}

/// A body that reads one span at a time.
///
/// An [`http_body::Body`] rather than a `Stream`: `futures_core` is gated on
/// `openapi32` and ranged delivery is not.
pub(super) struct Spans<S: ByteSource> {
    source: std::sync::Arc<S>,
    /// The next offset to read, and the last one enclosed.
    cursor: u64,
    last: u64,
    /// The read in flight, if any.
    reading: Option<Reading<S>>,
}

/// A `read_span` in flight, boxed because its future is opaque and held across
/// polls.
type Reading<S> = Pin<Box<dyn Future<Output = Result<Bytes, <S as ByteSource>::Error>> + Send>>;

#[allow(clippy::missing_fields_in_debug)]
impl<S: ByteSource> std::fmt::Debug for Spans<S> {
    /// Partial: the source and the pending read have nothing useful to print.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Spans")
            .field("cursor", &self.cursor)
            .field("last", &self.last)
            .field("reading", &self.reading.is_some())
            .finish()
    }
}

impl<S: ByteSource> Spans<S> {
    /// Reads `first..=last` from `source`, one [`SPAN`] at a time.
    pub(super) fn new(source: std::sync::Arc<S>, first: u64, last: u64) -> Self {
        Self {
            source,
            cursor: first,
            last,
            reading: None,
        }
    }

    /// The span the next read covers, or the one an empty answer failed to
    /// fill: neither offset moves while a read is in flight.
    fn span(&self) -> (u64, u64) {
        (
            self.cursor,
            self.last.min(self.cursor.saturating_add(SPAN - 1)),
        )
    }

    /// Whether every octet asked for has been read.
    fn exhausted(&self) -> bool {
        self.cursor > self.last
    }
}

impl<S: ByteSource> http_body::Body for Spans<S> {
    type Data = Bytes;
    type Error = crate::http::body::BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();

        loop {
            if let Some(reading) = this.reading.as_mut() {
                let read = std::task::ready!(reading.as_mut().poll(context));
                this.reading = None;

                return match read {
                    Ok(span) if span.is_empty() => {
                        // No progress: fail rather than end short of the sent
                        // length, and move past the end so a further poll ends.
                        let (first, last) = this.span();
                        this.cursor = this.last.saturating_add(1);
                        let truncated = Truncated { first, last };
                        Poll::Ready(Some(Err(Box::new(truncated) as Self::Error)))
                    }
                    Ok(span) => {
                        // A short answer: the remainder is asked for next poll.
                        this.cursor = this
                            .cursor
                            .saturating_add(u64::try_from(span.len()).unwrap_or(u64::MAX));
                        Poll::Ready(Some(Ok(http_body::Frame::data(span))))
                    }
                    Err(error) => Poll::Ready(Some(Err(Box::new(error) as Self::Error))),
                };
            }

            if this.exhausted() {
                return Poll::Ready(None);
            }

            let (first, last) = this.span();
            let source = std::sync::Arc::clone(&this.source);
            this.reading = Some(Box::pin(async move { source.read_span(first, last).await }));
        }
    }

    /// The exact length still to come, so the response carries a
    /// `Content-Length`; zero once exhausted, since the span is inclusive.
    fn size_hint(&self) -> http_body::SizeHint {
        if self.exhausted() {
            return http_body::SizeHint::with_exact(0);
        }

        http_body::SizeHint::with_exact(self.last.saturating_sub(self.cursor).saturating_add(1))
    }
}

#[cfg(test)]
mod tests;
