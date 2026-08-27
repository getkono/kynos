//! Where ranged octets come from.
//!
//! [`Rangeable`](super::rangeable) is the set of bodies a range can be *sliced*
//! from — octets already in hand, of a known length. This is the set it can be
//! *read* from: an object store, a fake filesystem in a test, a decrypting
//! reader, a file. The difference is that reading is asynchronous and partial,
//! so the complete representation never has to exist in memory at once.
//!
//! # The one rule
//!
//! **A source is asked for a span and returns that span.** It is never asked
//! for the whole representation, so serving a kilobyte out of a gigabyte costs
//! a kilobyte — which is most of the reason a range request exists. A source
//! that read everything and sliced would be honest about the octets and wrong
//! about the work.
//!
//! A source that answers with *fewer* octets than were asked for is simply
//! asked again for the remainder, so the body still delivers the whole span.
//! One that answers with *none* has said it cannot make progress at all, and
//! [`Truncated`] fails the body rather than ending it short of the length
//! already on the wire.
//!
//! # Not sealed
//!
//! `Rangeable` is sealed because its members are claims about what a byte range
//! *means*: a range of a `String` may split a character, and a range of a JSON
//! document is not a document. Those are closed questions. Where the octets
//! come from is not — an application knows storage Kynos never will, and
//! Beam's own requirement is a source that is "not only a filesystem path" so
//! that a fake one can stand in during tests.
//!
//! # Runtime-free
//!
//! Nothing here names tokio, which is what keeps
//! [`architecture.md`](../../../../docs/architecture.md)'s containment table at
//! the size it states. A source that reads a file names tokio *in the
//! implementation the application writes*, which is where a runtime belongs.

use std::{future::Future, pin::Pin, task::Poll};

use bytes::Bytes;

/// How much of a representation is read at a time.
///
/// A whole-representation response is streamed in spans of this size rather
/// than read at once, which is what makes "the full file is never buffered" a
/// property of the implementation rather than a promise. 64 KiB is large enough
/// that the per-span overhead disappears and small enough that a slow client
/// holding a connection open costs one span rather than one file.
pub const SPAN: u64 = 64 * 1024;

/// Octets a byte range can be read from.
///
/// Implement this over whatever holds the representation. The two methods are
/// deliberately the whole seam: anything richer — a seek cursor, a borrowed
/// reader, a transaction — has no portable equivalent across the stores this is
/// meant to reach, and would quietly make the trait implementable by one of
/// them.
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
    /// The application's own error, not one Kynos invented: it is the thing
    /// that knows whether a missing object is a 404 or a 500, and
    /// [`Served`](super::served::Served) hands it back rather than choosing.
    type Error: std::error::Error + Send + Sync + 'static;

    /// How many octets the whole representation has.
    ///
    /// Asked once per request, before anything is read. RFC 9110 section 14.1.2
    /// makes every offset relative to this, and section 14.4 asks a sender to
    /// state it — so a source that cannot answer cannot be ranged over, and an
    /// unsatisfiable request costs no read at all.
    fn complete_length(&self) -> impl Future<Output = Result<u64, Self::Error>> + Send;

    /// The octets from `first` to `last`, inclusive.
    ///
    /// Both offsets are within the length this source last reported, so an
    /// implementation does not have to bounds-check them against it.
    ///
    /// Returning fewer octets than were asked for is a short read and costs
    /// nothing but another call: the remainder is asked for on the next poll,
    /// and the body still fills the span its `Content-Range` names. Returning
    /// *none* is a failure -- it is the one answer that makes no progress, so
    /// the body cannot go on and cannot end without being shorter than a field
    /// section 14.4 tells a recipient never to recombine. It surfaces as
    /// [`Truncated`] on the body stream.
    fn read_span(
        &self,
        first: u64,
        last: u64,
    ) -> impl Future<Output = Result<Bytes, Self::Error>> + Send;
}

/// A source stopped short of the length it reported.
///
/// Raised when [`ByteSource::read_span`] answers a non-empty span with no
/// octets, which is the one short read that cannot be retried: a source with
/// fewer octets to give would give them, so one that gives none is telling the
/// body it can make no progress. A file or an object truncated between
/// [`complete_length`](ByteSource::complete_length) and the read is an ordinary
/// race for a filesystem or an object store, not a broken implementation.
///
/// It arrives on the body stream rather than as a status, because by then
/// [`Served::deliver`](super::served::Served::deliver) has already sent a
/// `Content-Length` -- and a `Content-Range` for a 206 -- sized from the
/// complete length. Cutting the body off mid-stream is what tells a recipient
/// the octets it holds are not the part it was promised; ending the body
/// quietly would hand it a truncated representation under a 200 or a 206.
///
/// A caller reads it back out of [`Body::Error`](http_body::Body::Error) by
/// downcasting the boxed error.
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
/// The degenerate source, and the one a test reaches for. It exists so that
/// `ByteSource` has an implementation in the crate that owns it — a trait whose
/// only implementations are downstream is one nothing here exercises.
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
/// An [`http_body::Body`] rather than a `Stream`, which is what keeps two
/// counted things where they were: `futures_core` reaches the tree only through
/// `openapi32`, and ranged delivery is behind no feature — and
/// [`architecture.md`](../../../../docs/architecture.md) enumerates every
/// hand-rolled `Stream` in the crate, so a fourth would have to be argued for.
/// A body is what this actually is: `http-body` is already an unconditional
/// dependency, and `Body::from_body` already exists to erase one.
///
/// The pending read is boxed because a `ByteSource`'s future is opaque and has
/// to be held across polls. That is one allocation per 64 KiB of body, and it
/// appears in no public signature.
pub(super) struct Spans<S: ByteSource> {
    source: std::sync::Arc<S>,
    /// The next offset to read, and the last one enclosed.
    cursor: u64,
    last: u64,
    /// The read in flight, if any.
    reading: Option<Reading<S>>,
}

/// A `read_span` in flight.
///
/// Boxed because a [`ByteSource`]'s future is opaque and has to be held across
/// polls, and named because the type written out is not one a reader should
/// have to parse.
type Reading<S> = Pin<Box<dyn Future<Output = Result<Bytes, <S as ByteSource>::Error>> + Send>>;

#[allow(clippy::missing_fields_in_debug)]
impl<S: ByteSource> std::fmt::Debug for Spans<S> {
    /// Hand-written, and deliberately partial: the source is opaque and the
    /// pending read is a future, neither of which has a `Debug` or anything
    /// useful to print if it had one. What is left is the position, which is
    /// the whole of this type's state.
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

    /// The span the next read covers, which is also the one an empty answer
    /// failed to fill -- neither offset moves while a read is in flight, so the
    /// two callers see the same pair.
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
                        // Nothing came back for a span that is still owed, so
                        // the source has said it can make no progress. The head
                        // is long gone and named a length this body can no
                        // longer reach, so it fails rather than ending short --
                        // and the cursor is moved past the end so a driver that
                        // polls on regardless is answered once and then ended,
                        // never spun.
                        let (first, last) = this.span();
                        this.cursor = this.last.saturating_add(1);
                        let truncated = Truncated { first, last };
                        Poll::Ready(Some(Err(Box::new(truncated) as Self::Error)))
                    }
                    Ok(span) => {
                        // A short answer is not a failure: the cursor advances
                        // by what arrived and the remainder is asked for on the
                        // next poll, so the span is still filled in full.
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

    /// The exact length still to come, which a caller already fixed from
    /// `complete_length`.
    ///
    /// Stated so the response carries a `Content-Length` rather than a chunked
    /// encoding: section 14.4 asks a 206 to name the part it encloses, and a
    /// client sizing a download reads the field rather than counting octets.
    ///
    /// Zero once the cursor has passed the last offset. The span is inclusive,
    /// so the remaining count is one more than the difference -- but an
    /// exhausted body has no remainder to add one to, and reporting one octet
    /// that will never arrive is how a caller ends up waiting for it.
    fn size_hint(&self) -> http_body::SizeHint {
        if self.exhausted() {
            return http_body::SizeHint::with_exact(0);
        }

        http_body::SizeHint::with_exact(self.last.saturating_sub(self.cursor).saturating_add(1))
    }
}

#[cfg(test)]
mod tests;
