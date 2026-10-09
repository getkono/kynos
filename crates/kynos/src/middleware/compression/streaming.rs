//! Encoding a body whose length nobody knows until it ends.
//!
//! Where the buffered path in [`super`] collects a response and encodes it
//! once, this encodes as the frames arrive.

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
/// `#[non_exhaustive]`: Kynos may add a mode without a breaking change.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LatencyMode {
    /// Flush after every frame the handler produces.
    ///
    /// The default: an event stream, log tail or progress feed is read as it is
    /// produced, and under [`Throughput`](LatencyMode::Throughput) an idle one
    /// can go minutes without the client seeing an event it was sent.
    ///
    /// It costs ratio: a flush closes the current block, so a stream of small
    /// frames compresses worse than the same bytes in one piece.
    #[default]
    Interactive,
    /// Let the compressor fill its window before emitting anything.
    ///
    /// The better ratio, for a body that is a stream only because it is large —
    /// a file, an export, a database dump.
    Throughput,
}

/// One of the three write-side encoders, over a buffer it fills. Write-side
/// because only it can flush, which [`LatencyMode::Interactive`] needs.
enum Encoder {
    // Boxed: codec state runs to tens of kilobytes, held for the whole exchange.
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
    /// The inner body or the encoder failed: nothing more is yielded, and the
    /// body did not end.
    Failed,
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

    /// Yields `error` and ends the body as failed; the only way an error leaves
    /// this body, so every failure is fused.
    fn fail(&mut self, error: BoxError) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        self.state = State::Failed;
        Poll::Ready(Some(Err(error)))
    }
}

/// The encoder's answer to a write of pending bytes, with `Ok(0)` turned into
/// `WriteZero` since polling again would spin forever.
fn accepted(result: io::Result<usize>) -> io::Result<usize> {
    match result {
        Ok(0) => Err(io::Error::from(io::ErrorKind::WriteZero)),
        other => other,
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
                    // Trailers go after the last of the coded stream.
                    return Poll::Ready(this.trailers.take().map(Ok));
                }

                // Held trailers are never yielded after a failure.
                State::Failed => return Poll::Ready(None),

                State::Finishing => {
                    if let Err(error) =
                        ready!(this.encoder.with(|encoder| encoder.poll_shutdown(context)))
                    {
                        return this.fail(Box::new(error));
                    }
                    this.state = State::Done;

                    if let Some(frame) = this.emit() {
                        return Poll::Ready(Some(Ok(frame)));
                    }
                }

                State::Flushing => {
                    if let Err(error) =
                        ready!(this.encoder.with(|encoder| encoder.poll_flush(context)))
                    {
                        return this.fail(Box::new(error));
                    }
                    this.state = State::Feeding;

                    if let Some(frame) = this.emit() {
                        return Poll::Ready(Some(Ok(frame)));
                    }
                }

                State::Feeding => {
                    // The rest of the last frame goes in first; partial writes
                    // are ordinary.
                    if !this.pending.is_empty() {
                        let written = match accepted(ready!(
                            this.encoder
                                .with(|encoder| encoder.poll_write(context, &this.pending))
                        )) {
                            Ok(written) => written,
                            Err(error) => return this.fail(Box::new(error)),
                        };

                        this.pending.advance(written);

                        if this.pending.is_empty() && this.latency == LatencyMode::Interactive {
                            this.state = State::Flushing;
                            continue;
                        }

                        // Under `Throughput`, send only what the codec emitted.
                        if let Some(frame) = this.emit() {
                            return Poll::Ready(Some(Ok(frame)));
                        }

                        continue;
                    }

                    match ready!(Pin::new(&mut this.inner).poll_frame(context)) {
                        None => this.state = State::Finishing,
                        Some(Err(error)) => return this.fail(error),
                        Some(Ok(frame)) => match frame.into_data() {
                            Ok(data) => this.pending = data,
                            // Trailers, held until the coded stream finishes.
                            Err(other) => this.trailers = Some(other),
                        },
                    }
                }
            }
        }
    }

    // Never `Failed`: `Watched` reads this on drop to report delivery, and a
    // failed body did not end.
    fn is_end_stream(&self) -> bool {
        self.state == State::Done && self.trailers.is_none()
    }

    /// Unknown until the encoding finishes, so the driver frames the response
    /// per RFC 9112 section 6.1 rather than with a `Content-Length`.
    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

#[cfg(test)]
mod tests;
