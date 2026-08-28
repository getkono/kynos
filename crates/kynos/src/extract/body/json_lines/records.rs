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
};

/// The record separator RFC 7464 puts before each JSON text.
///
/// It cannot occur inside a JSON text, which is the whole reason the framing
/// exists: a value holding a newline stays one record.
///
/// One spelling, read by both halves: the byte this decoder scans for is the
/// byte the responding half of this codec writes in
/// front of every record it emits.
pub(crate) const RECORD_SEPARATOR: u8 = 0x1e;

/// Which bytes separate one record from the next.
///
/// Carried as a field rather than as a type parameter, because the two framings
/// differ in a delimiter and in where it sits — not in anything a caller of
/// [`Records`] can observe.
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
/// as a `futures_core::Stream`. The inherent methods are there so that reading
/// needs no combinator crate: Kynos depends on `futures-core`, not on
/// `futures-util`.
///
/// # What a failure costs
///
/// The item type is `Result<T, BodyRejection>`, and the rejection is the one
/// every body codec already raises. That is the whole soundness argument: an
/// extractor's `Rejection` is already merged into the operation's responses, so
/// every status a mid-stream failure can produce is already declared and a
/// handler answers with one by returning it.
///
/// Unlike the response half, nothing is committed when a record fails. A
/// handler returns a `Response` *value* and nothing reaches the socket until
/// its future resolves, so a 422 raised on the last record of a long body is
/// still a 422. The asymmetry is the reason the two halves behave differently
/// at all: a response stream has spent its status by the time it meets a bad
/// item, and a request stream has not.
///
/// | The record | Answer | Afterwards |
/// | --- | --- | --- |
/// | is not well-formed JSON | 400 | the stream ends — after a framing failure the record boundaries are no longer trustworthy |
/// | is JSON that does not fit `T` | 422 at JSON Pointer `/{index}` | the stream continues — the boundaries held, so a bulk ingest can report every bad record at once |
/// | did not arrive, because the transport failed | 400 | the stream ends |
/// | opens a `json-seq` body without a record separator | 400 | the stream ends |
///
/// The pointer is the record's index in the body, which OpenAPI 3.2 makes well
/// defined: an implementation reads a sequential media type as if the values
/// were an array in the same order, so `/3` names the fourth record.
///
/// [`BodyRejection`] is deliberately not `Serialize`, so `JsonLines<Records<T>>`
/// is not [`IntoResponse`](crate::response::IntoResponse) and piping a request
/// stream straight into a streaming response does not typecheck. That is the
/// one arrangement where a failed record genuinely would have no status left to
/// spend.
///
/// # Empty records are skipped
///
/// A blank line, or two adjacent record separators, produce no item and no
/// rejection. This is forced rather than chosen: reading forwards, a decoder
/// cannot tell the trailing separator a writer is permitted to emit — and which
/// Kynos's own [`JsonLines`](super::JsonLines) response does emit — from an interior blank,
/// without buffering past it and giving up the streaming it exists for.
///
/// # What a chunked body costs
///
/// [`BodySize`](crate::middleware::limits::BodySize) and streaming do not
/// compose all the way. A request declaring a `Content-Length` passes the limit
/// untouched and streams. A chunked request declares no length, so a running
/// count is the only bound there is — and the limit materialises the whole body
/// before the handler is entered. Records still arrive one at a time, but
/// nothing is saved. `Records` adds no cap of its own and no status of its own;
/// `docs/nfr.md` records the limit beside HTTP/2 flow control, which is the
/// same family of fact.
pub struct Records<T> {
    /// The undecoded body, as the frames it arrives in.
    body: BodyDataStream<Body>,
    /// Bytes read but not yet framed into a record.
    buffer: BytesMut,
    /// How far into `buffer` the search for a delimiter has already reached, so
    /// a record spanning many frames is scanned once rather than once a frame.
    scanned: usize,
    /// How many records have been decoded, which is the JSON Pointer a schema
    /// failure is reported at.
    index: usize,
    framing: Framing,
    state: State,
    /// `fn() -> T` rather than `T`, so that `Records<T>` is `Send` whatever `T`
    /// is: the decoder produces a `T` and never holds one.
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
    /// Enforces the content type, then takes the body unread.
    ///
    /// The check happens before a byte is read, which is what keeps an
    /// operation from accepting a media type its description never claimed —
    /// the same 415 every other codec produces, from the same two helpers.
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
            body: request.into_body().into_data_stream(),
            buffer: BytesMut::new(),
            scanned: 0,
            index: 0,
            framing,
            state: State::Reading,
            item: PhantomData,
        })
    }

    /// The bytes of the next record, or `None` when the buffer holds none.
    ///
    /// Empty records are consumed here rather than reported, so what this
    /// returns is always something to decode.
    fn next_frame(&mut self) -> Option<Result<Bytes, BodyRejection>> {
        let (prefix, delimiter) = match self.framing {
            Framing::Lines => (0, b'\n'),
            Framing::Sequence => (1, RECORD_SEPARATOR),
        };

        loop {
            if self.buffer.is_empty() {
                return None;
            }

            // RFC 7464 makes the separator a prefix, so a body that does not
            // open with one is not a sequence and nothing after this point can
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

            let mut frame = match found {
                // The delimiter belongs to the framing rather than to the
                // record: a newline ends this one, a separator begins the next.
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

            // Whatever the framing put in front of the record is not part of it.
            let _ = frame.split_to(prefix.min(frame.len()));

            let record = trimmed(frame.freeze());
            if record.is_empty() {
                continue;
            }
            return Some(Ok(record));
        }
    }
}

impl<T: serde::de::DeserializeOwned> Records<T> {
    /// The next record, or `None` once the body has no more to give.
    ///
    /// An inherent method rather than a combinator, so that reading a body
    /// needs no dependency an application would not otherwise have.
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
                // What arrived is not the body the client meant to send, which
                // is the 400 a whole-body read answers with for the same
                // reason. Nothing after the break is a record.
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

    /// Decodes one record, drawing the 400/422 line where every JSON body
    /// draws it.
    fn decode(&mut self, record: &[u8]) -> Result<T, BodyRejection> {
        let index = self.index;
        self.index += 1;

        serde_json::from_slice(record).map_err(|error| {
            if crate::extract::body::json::is_schema_failure(&error) {
                // The record was a record; only its shape was wrong. The
                // boundaries held, so reading continues.
                BodyRejection::Schema {
                    failures: BTreeMap::from([(format!("/{index}"), error.to_string())]),
                }
            } else {
                self.state = State::Fused;
                BodyRejection::Syntax {
                    detail: format!("record {index}: {error}"),
                }
            }
        })
    }
}

/// The record inside a frame, without the whitespace around it.
///
/// Trimming is what makes a `\r\n` line ending and RFC 7464's trailing newline
/// the same non-event, and it is what decides a record is empty.
fn trimmed(frame: Bytes) -> Bytes {
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

/// The one hand-rolled `Stream` in the crate.
///
/// Every field is `Unpin` — the body is, and so is the buffer — so this needs
/// no projection and no `unsafe`, which is forbidden here.
impl<T: serde::de::DeserializeOwned> Stream for Records<T> {
    type Item = Result<T, BodyRejection>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().poll_record(context)
    }
}
