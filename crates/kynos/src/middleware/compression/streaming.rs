//! Encoding a body whose length nobody knows until it ends.
//!
//! The buffered path in [`super`] collects a response and encodes it once,
//! which is right when the length is already known and impossible when it is
//! not: a body still being produced cannot be collected without waiting for a
//! producer that may never stop. This encodes as the frames arrive.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};

use async_compression::{
    Level,
    tokio::write::{BrotliEncoder, GzipEncoder, ZstdEncoder},
};
use bytes::{Buf, Bytes};
use http_body::{Body as HttpBody, Frame, SizeHint};
use tokio::io::AsyncWrite;

use crate::middleware::compression::{Coding, Levels, as_level};

/// The error a body reports, whatever produced it.
type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// How eagerly encoded bytes are handed to the client.
///
/// Only meaningful for a body being produced as it goes. A response whose
/// length is already known is encoded in one pass, and there is nothing to
/// trade.
///
/// `#[non_exhaustive]`, for the reason
/// [`Encoding`](crate::middleware::compression::policy::Encoding)'s is: the set
/// is Kynos's, and a third mode — a size threshold, say — is a decision this
/// crate may take without it being a breaking change downstream.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LatencyMode {
    /// Flush after every frame the handler produces.
    ///
    /// The default, and deliberately not the one that compresses best. A body
    /// the server is producing incrementally is one whose reader is consuming
    /// it incrementally — an event stream, a log tail, a progress feed — and
    /// withholding those bytes to fill a compression window does not slow the
    /// response down so much as break it. Under
    /// [`Throughput`](LatencyMode::Throughput) an idle event stream can go
    /// minutes without the client seeing an event it was sent immediately.
    ///
    /// It costs ratio: a flush closes the current block, so a stream of small
    /// frames compresses worse than the same bytes in one piece.
    #[default]
    Interactive,
    /// Let the compressor fill its window before emitting anything.
    ///
    /// The better ratio, and the right choice for a body that is a stream only
    /// because it is large — a file, an export, a database dump — where nobody
    /// is reading it a record at a time.
    Throughput,
}

/// One of the three write-side encoders, over a buffer it fills.
///
/// Write-side rather than the read-side encoders the buffered path uses,
/// because only this side has a flush: the read-side encoders hold whatever the
/// codec has not decided to emit, which is exactly the behaviour
/// [`LatencyMode::Interactive`] exists to prevent.
enum Encoder {
    // Each is boxed: the codec state is measured in kilobytes -- tens of them
    // for brotli -- and this sits inside a response body held for the whole
    // exchange.
    Gzip(Box<GzipEncoder<Vec<u8>>>),
    Brotli(Box<BrotliEncoder<Vec<u8>>>),
    Zstd(Box<ZstdEncoder<Vec<u8>>>),
}

impl Encoder {
    /// An encoder for `coding` at the level `levels` sets for it.
    fn new(coding: Coding, levels: Levels) -> Self {
        match coding {
            Coding::Gzip => Self::Gzip(Box::new(GzipEncoder::with_quality(
                Vec::new(),
                Level::Precise(as_level(levels.gzip.get())),
            ))),
            Coding::Brotli => Self::Brotli(Box::new(BrotliEncoder::with_quality(
                Vec::new(),
                Level::Precise(as_level(levels.brotli.get())),
            ))),
            Coding::Zstd => Self::Zstd(Box::new(ZstdEncoder::with_quality(
                Vec::new(),
                Level::Precise(levels.zstd.get()),
            ))),
        }
    }

    /// Applies `operation` to whichever encoder this is.
    ///
    /// One place the three variants are unified, so the polling below reads as
    /// the state machine it is rather than as three copies of it.
    fn with<T>(
        &mut self,
        operation: impl FnOnce(Pin<&mut (dyn AsyncWrite + Send + Unpin)>) -> T,
    ) -> T {
        match self {
            Self::Gzip(encoder) => operation(Pin::new(encoder.as_mut())),
            Self::Brotli(encoder) => operation(Pin::new(encoder.as_mut())),
            Self::Zstd(encoder) => operation(Pin::new(encoder.as_mut())),
        }
    }

    /// The bytes encoded so far, taken out.
    fn take(&mut self) -> Bytes {
        let buffer = match self {
            Self::Gzip(encoder) => encoder.get_mut(),
            Self::Brotli(encoder) => encoder.get_mut(),
            Self::Zstd(encoder) => encoder.get_mut(),
        };

        Bytes::from(std::mem::take(buffer))
    }
}

/// What the body is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Taking frames from the handler and feeding them to the encoder.
    Feeding,
    /// Closing the current block so what has been written can be sent.
    Flushing,
    /// The handler is done; finishing the coded stream.
    Finishing,
    /// Everything has been yielded.
    Done,
}

/// A body that encodes another as its frames arrive.
pub(crate) struct Streamed {
    inner: crate::http::body::Body,
    encoder: Encoder,
    latency: LatencyMode,
    state: State,
    /// Read from the handler and not yet handed to the encoder.
    pending: Bytes,
    /// Held back until the coded stream is finished, since trailers are last.
    trailers: Option<Frame<Bytes>>,
}

impl Streamed {
    /// Encodes `inner` under `coding`.
    pub(crate) fn new(
        inner: crate::http::body::Body,
        coding: Coding,
        levels: Levels,
        latency: LatencyMode,
    ) -> Self {
        Self {
            inner,
            encoder: Encoder::new(coding, levels),
            latency,
            state: State::Feeding,
            pending: Bytes::new(),
            trailers: None,
        }
    }

    /// The encoded bytes so far as a frame, if there are any.
    fn emit(&mut self) -> Option<Frame<Bytes>> {
        let encoded = self.encoder.take();
        (!encoded.is_empty()).then(|| Frame::data(encoded))
    }
}

impl HttpBody for Streamed {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();

        loop {
            match this.state {
                State::Done => {
                    // Trailers describe the whole body, so they go after the
                    // last of it rather than where they arrived.
                    return Poll::Ready(this.trailers.take().map(Ok));
                }

                State::Finishing => {
                    ready!(this.encoder.with(|encoder| encoder.poll_shutdown(context)))?;
                    this.state = State::Done;

                    if let Some(frame) = this.emit() {
                        return Poll::Ready(Some(Ok(frame)));
                    }
                }

                State::Flushing => {
                    ready!(this.encoder.with(|encoder| encoder.poll_flush(context)))?;
                    this.state = State::Feeding;

                    if let Some(frame) = this.emit() {
                        return Poll::Ready(Some(Ok(frame)));
                    }
                }

                State::Feeding => {
                    // Whatever is left of the last frame goes in first. A
                    // partial write is ordinary rather than exceptional: the
                    // encoder's own buffer decides how much it takes.
                    if !this.pending.is_empty() {
                        let written = ready!(
                            this.encoder
                                .with(|encoder| encoder.poll_write(context, &this.pending))
                        )?;

                        // A writer that accepts nothing would spin here
                        // forever, so treat it as the broken writer it is.
                        if written == 0 {
                            this.state = State::Done;
                            return Poll::Ready(Some(Err(Box::new(io::Error::from(
                                io::ErrorKind::WriteZero,
                            )))));
                        }

                        this.pending.advance(written);

                        if this.pending.is_empty() && this.latency == LatencyMode::Interactive {
                            this.state = State::Flushing;
                            continue;
                        }

                        // Under `Throughput` nothing is forced out, so this
                        // sends whatever the codec decided to emit on its own
                        // and otherwise reads on.
                        if let Some(frame) = this.emit() {
                            return Poll::Ready(Some(Ok(frame)));
                        }

                        continue;
                    }

                    match ready!(Pin::new(&mut this.inner).poll_frame(context)) {
                        None => this.state = State::Finishing,
                        Some(Err(error)) => {
                            this.state = State::Done;
                            return Poll::Ready(Some(Err(error)));
                        }
                        Some(Ok(frame)) => match frame.into_data() {
                            Ok(data) => this.pending = data,
                            // Not data, so it is trailers. Held rather than
                            // forwarded: the coded stream is not finished, and
                            // trailers after which more body arrives are not
                            // trailers.
                            Err(other) => this.trailers = Some(other),
                        },
                    }
                }
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.state == State::Done && self.trailers.is_none()
    }

    /// Deliberately unknown.
    ///
    /// The encoded length is not known until the encoding is finished, and RFC
    /// 9110 section 8.6 forbids forwarding a `Content-Length` known to be
    /// incorrect. An unknown hint is what lets the protocol driver frame the
    /// response the way RFC 9112 section 6.1 asks for instead.
    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

#[cfg(test)]
mod tests;
