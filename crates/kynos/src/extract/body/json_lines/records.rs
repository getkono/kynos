//! Framing a streamed JSON request body into the records it carries.

use std::{
    collections::BTreeMap,
    fmt,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use futures_core::Stream;
use http_body_util::{BodyDataStream, BodyExt};

use crate::{
    error::rejection::BodyRejection,
    extract::body::json_lines::SEQUENCE_MEDIA_TYPE,
    http::{Request, body::Body},
    schema::{Schema, constraints::Pointer},
};

/// The record separator RFC 7464 puts before each JSON text; shared with the
/// responding half of this codec.
pub(crate) const RECORD_SEPARATOR: u8 = 0x1e;

/// Which bytes separate one record from the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Framing {
    /// A newline *after* each record, which NDJSON writes.
    Lines,
    /// RFC 7464's record separator *before* each record.
    Sequence,
}

/// How much more of the body there is to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// More bytes may still arrive.
    Reading,
    /// The body ended; whatever is left in the buffer is the last record.
    Ended,
    /// Nothing further will be produced.
    Fused,
}

/// A streamed JSON request body, decoded one record at a time.
///
/// The `items` of a [`JsonLines`](super::JsonLines) or [`JsonSeq`](super::JsonSeq) read from a request. Nothing
/// is read before the handler asks for it: extraction enforces the
/// `Content-Type` and then awaits nothing, so a body of any length reaches the
/// handler as soon as the head does.
///
/// Read it with [`next`](Records::next) or [`read_all`](Records::read_all), or
/// as a `futures_core::Stream`; the inherent methods need no combinator crate.
///
/// # What a failure costs
///
/// The item type is `Result<T, BodyRejection>`, the rejection every body codec
/// raises, so every status a mid-stream failure can produce is already in the
/// operation's description and a handler answers with one by returning it.
/// Nothing reaches the socket until the handler returns, so a 422 on the last
/// record of a long body is still a 422.
///
/// | The record | Answer | Afterwards |
/// | --- | --- | --- |
/// | is not well-formed JSON | 400 | the stream ends — after a framing failure the record boundaries are no longer trustworthy |
/// | is JSON that does not fit `T` | 422 at JSON Pointer `/{index}` | the stream continues — the boundaries held, so a bulk ingest can report every bad record at once |
/// | breaks a bound `T`'s schema declares | 422 at each offending member, under `/{index}` | the stream continues, for the same reason |
/// | did not arrive, because the transport failed | 400 | the stream ends |
/// | opens a `json-seq` body without a record separator | 400 | the stream ends |
/// | is longer than the operation's body limit | 413 | the stream ends — the rest of the record was never read, so nothing after it can be framed |
///
/// The pointer is the record's index in the body, as OpenAPI 3.2 reads a
/// sequential media type as an array: `/3` names the fourth record.
///
/// [`BodyRejection`] is not `Serialize`, so `JsonLines<Records<T>>` is not
/// [`IntoResponse`](crate::response::IntoResponse): a request stream cannot be
/// piped into a streaming response, where a failed record would have no status
/// left to spend.
///
/// # Empty records are skipped
///
/// A blank line, or two adjacent record separators, produce no item and no
/// rejection: a streaming decoder cannot tell a permitted trailing separator
/// from an interior blank.
///
/// # What a chunked body costs
///
/// A request declaring a `Content-Length` passes
/// [`BodySize`](crate::middleware::limits::body_size::BodySize) untouched and
/// streams. A chunked request is materialised whole by that limit before the
/// handler is entered, so records still arrive one at a time but nothing is
/// saved.
///
/// # What one record may cost
///
/// The body as a whole is unbounded, but each record is held whole before
/// decoding, so the operation's body limit —
/// [`DEFAULT_LIMIT`](crate::extract::body::limit::DEFAULT_LIMIT), or the figure
/// a covering `BodySize` names — bounds each record, with a 413.
pub struct Records<T> {
    /// The undecoded body, as the frames it arrives in.
    body: BodyDataStream<Body>,
    /// The longest record, in bytes, that will be held for decoding.
    limit: u64,
    /// Bytes read but not yet framed into a record.
    buffer: BytesMut,
    /// How far into `buffer` the delimiter search has reached, so a record
    /// spanning many frames is scanned once.
    scanned: usize,
    /// How many records have been decoded: the JSON Pointer of a failure.
    index: usize,
    framing: Framing,
    state: State,
    /// `fn() -> T` so `Records<T>` is `Send` whatever `T` is.
    item: PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for Records<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Records")
            .field("framing", &self.framing)
            .field("state", &self.state)
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl<T> Records<T> {
    /// Enforces the content type (the shared 415), then takes the body unread.
    pub(super) fn new(
        request: Request,
        media_type: &str,
        framing: Framing,
    ) -> Result<Self, BodyRejection> {
        if !crate::extract::body::offers(request.headers(), media_type) {
            return Err(crate::extract::body::unsupported_media_type(
                request.headers(),
            ));
        }

        Ok(Self {
            limit: crate::extract::body::limit::of(&request),
            body: request.into_body().into_data_stream(),
            buffer: BytesMut::new(),
            scanned: 0,
            index: 0,
            framing,
            state: State::Reading,
            item: PhantomData,
        })
    }

    /// The bytes of the next non-empty record, or `None` when the buffer holds
    /// none.
    fn next_frame(&mut self) -> Option<Result<Bytes, BodyRejection>> {
        let (prefix, delimiter) = match self.framing {
            Framing::Lines => (0, b'\n'),
            Framing::Sequence => (1, RECORD_SEPARATOR),
        };

        loop {
            if self.buffer.is_empty() {
                return None;
            }

            // RFC 7464 makes the separator a prefix; without one nothing can
            // be framed.
            if self.framing == Framing::Sequence && self.buffer[0] != RECORD_SEPARATOR {
                self.state = State::Fused;
                return Some(Err(BodyRejection::Syntax {
                    detail: format!(
                        "an `{SEQUENCE_MEDIA_TYPE}` body must begin with a record separator"
                    ),
                }));
            }

            let from = self.scanned.max(prefix);
            let found = self.buffer[from..]
                .iter()
                .position(|byte| *byte == delimiter)
                .map(|position| from + position);

            // A record still arriving is refused as soon as it passes the limit.
            let held = found.unwrap_or(self.buffer.len()).saturating_sub(prefix);
            if u64::try_from(held).unwrap_or(u64::MAX) > self.limit {
                self.state = State::Fused;
                return Some(Err(BodyRejection::TooLarge { limit: self.limit }));
            }

            let mut frame = match found {
                // A newline ends this record; a separator begins the next.
                Some(end) => {
                    let taken = match self.framing {
                        Framing::Lines => end + 1,
                        Framing::Sequence => end,
                    };
                    let mut frame = self.buffer.split_to(taken);
                    frame.truncate(end);
                    self.scanned = 0;
                    frame
                }
                None if self.state == State::Ended => {
                    self.scanned = 0;
                    std::mem::take(&mut self.buffer)
                }
                None => {
                    self.scanned = self.buffer.len();
                    return None;
                }
            };

            let _ = frame.split_to(prefix.min(frame.len()));

            let record = trimmed(&frame.freeze());
            if record.is_empty() {
                continue;
            }
            return Some(Ok(record));
        }
    }
}

/// `T: Schema` because each record is held to the bounds its schema declares,
/// as a [`Json`](crate::extract::body::json::Json) body is.
impl<T: serde::de::DeserializeOwned + Schema> Records<T> {
    /// The next record, or `None` once the body has no more to give.
    pub async fn next(&mut self) -> Option<Result<T, BodyRejection>> {
        std::future::poll_fn(|context| self.poll_record(context)).await
    }

    /// Every remaining record, or the first failure.
    ///
    /// The convenience for a body that fits in memory: it returns at the first
    /// rejection, including the 422 that [`next`](Records::next) would have
    /// carried on past, and drops what has not been read.
    pub async fn read_all(mut self) -> Result<Vec<T>, BodyRejection> {
        let mut records = Vec::new();
        while let Some(record) = self.next().await {
            records.push(record?);
        }
        Ok(records)
    }

    /// One record: framed from the buffer, refilled from the body when the
    /// buffer holds no whole one yet.
    fn poll_record(&mut self, context: &mut Context<'_>) -> Poll<Option<Result<T, BodyRejection>>> {
        loop {
            if self.state == State::Fused {
                return Poll::Ready(None);
            }

            match self.next_frame() {
                Some(Ok(record)) => return Poll::Ready(Some(self.decode(&record))),
                Some(Err(rejection)) => return Poll::Ready(Some(Err(rejection))),
                None if self.state == State::Ended => return Poll::Ready(None),
                None => {}
            }

            match Pin::new(&mut self.body).poll_next(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) => self.buffer.extend_from_slice(&chunk),
                // A transport failure is a 400, as for a whole-body read.
                Poll::Ready(Some(Err(error))) => {
                    self.state = State::Fused;
                    return Poll::Ready(Some(Err(BodyRejection::Syntax {
                        detail: error.to_string(),
                    })));
                }
                Poll::Ready(None) => self.state = State::Ended,
            }
        }
    }

    /// Decodes one record and holds it to the bounds `T` declares, drawing
    /// the 400/422 line where every JSON body draws it.
    fn decode(&mut self, record: &[u8]) -> Result<T, BodyRejection> {
        let index = self.index;
        self.index += 1;

        let value = serde_json::from_slice(record).map_err(|error| {
            if crate::extract::body::json::is_schema_failure(&error) {
                // Only the shape was wrong; the boundaries held, so reading
                // continues.
                BodyRejection::Schema {
                    failures: BTreeMap::from([(format!("/{index}"), error.to_string())]),
                }
            } else {
                self.state = State::Fused;
                BodyRejection::Syntax {
                    detail: format!("record {index}: {error}"),
                }
            }
        })?;

        // A broken bound is a shape failure too, so reading continues past it.
        crate::extract::body::checked(value, Pointer::root().index(index))
    }
}

/// The record inside a frame, without the whitespace around it.
///
/// Absorbs a `\r\n` line ending and RFC 7464's trailing newline.
fn trimmed(frame: &Bytes) -> Bytes {
    let start = frame
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(frame.len());
    let end = frame
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |position| position + 1);

    frame.slice(start..end)
}

/// Every field is `Unpin`, so this needs no projection and no `unsafe`.
impl<T: serde::de::DeserializeOwned + Schema> Stream for Records<T> {
    type Item = Result<T, BodyRejection>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().poll_record(context)
    }
}
