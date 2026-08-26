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
mod tests {
    use std::collections::VecDeque;

    use async_compression::tokio::bufread::GzipDecoder;
    use http_body_util::BodyExt as _;
    use tokio::io::AsyncReadExt as _;

    use super::{
        Bytes, Coding, Context, Frame, HttpBody, LatencyMode, Levels, Pin, Poll, SizeHint, Streamed,
    };

    /// A body that yields the frames it was given and states no length.
    ///
    /// The shape this file exists for: a handler producing bytes as it goes.
    /// `size_hint` is deliberately unknown, since a body that could state its
    /// length would take the buffered path instead.
    struct Frames(VecDeque<Bytes>);

    impl HttpBody for Frames {
        type Data = Bytes;
        type Error = Box<dyn std::error::Error + Send + Sync>;

        fn poll_frame(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
            Poll::Ready(
                self.get_mut()
                    .0
                    .pop_front()
                    .map(|data| Ok(Frame::data(data))),
            )
        }

        fn size_hint(&self) -> SizeHint {
            SizeHint::default()
        }
    }

    /// The frames a handler produces, as a body Kynos can hand on.
    fn producing(frames: &[&str]) -> crate::http::body::Body {
        crate::http::body::Body::from_body(Frames(
            frames
                .iter()
                .map(|text| Bytes::from(text.to_string()))
                .collect(),
        ))
    }

    /// Encodes `frames` and reports each encoded frame, in order.
    async fn encoded(frames: &[&str], latency: LatencyMode) -> Vec<Bytes> {
        let mut body = Streamed::new(producing(frames), Coding::Gzip, Levels::default(), latency);
        let mut produced = Vec::new();

        while let Some(frame) = std::pin::Pin::new(&mut body).frame().await {
            if let Ok(data) = frame.expect("an encoded frame").into_data() {
                produced.push(data);
            }
        }

        produced
    }

    /// What a client gets back after decoding.
    async fn decoded(encoded: &[Bytes]) -> String {
        let joined: Vec<u8> = encoded.iter().flat_map(|chunk| chunk.to_vec()).collect();
        let mut text = String::new();

        GzipDecoder::new(std::io::Cursor::new(joined))
            .read_to_string(&mut text)
            .await
            .expect("a well-formed gzip stream");

        text
    }

    const FRAMES: &[&str] = &[
        "the first thing the handler produced\n",
        "the second thing the handler produced\n",
        "the third thing the handler produced\n",
    ];

    /// The property everything else rests on: what arrives is what was sent.
    /// A flush in the wrong place produces a stream that decodes to less than
    /// it was given, or to nothing at all.
    #[tokio::test]
    async fn a_streamed_body_decodes_to_exactly_what_the_handler_produced() {
        for latency in [LatencyMode::Interactive, LatencyMode::Throughput] {
            let produced = encoded(FRAMES, latency).await;

            assert_eq!(
                decoded(&produced).await,
                FRAMES.concat(),
                "the stream did not round-trip under {latency:?}"
            );
        }
    }

    /// The whole of the latency trade, in one comparison. Interactive closes a
    /// block per frame so the reader sees each one; throughput lets the codec
    /// hold them until it has a window's worth.
    #[tokio::test]
    async fn interactive_sends_the_frames_as_they_arrive_and_throughput_does_not() {
        let interactive = encoded(FRAMES, LatencyMode::Interactive).await;
        let throughput = encoded(FRAMES, LatencyMode::Throughput).await;

        assert!(
            interactive.len() >= FRAMES.len(),
            "interactive produced {} frames for {} the handler sent, so a reader \
             waiting on the first was made to wait for a later one",
            interactive.len(),
            FRAMES.len()
        );
        assert!(
            throughput.len() < interactive.len(),
            "throughput produced {} frames and interactive {}, so the two modes \
             are the same mode",
            throughput.len(),
            interactive.len()
        );
    }

    /// The cost of the trade, stated rather than assumed. If flushing were
    /// free there would be no reason to offer the other mode.
    #[tokio::test]
    async fn interactive_costs_ratio() {
        let interactive: usize = encoded(FRAMES, LatencyMode::Interactive)
            .await
            .iter()
            .map(Bytes::len)
            .sum();
        let throughput: usize = encoded(FRAMES, LatencyMode::Throughput)
            .await
            .iter()
            .map(Bytes::len)
            .sum();

        assert!(
            throughput < interactive,
            "throughput sent {throughput} bytes and interactive {interactive}"
        );
    }

    /// The encoded length is not known until the encoding finishes, which is
    /// after the head has gone. RFC 9110 section 8.6 forbids forwarding a
    /// `Content-Length` known to be incorrect, and an exact hint here is how
    /// one would be derived.
    #[test]
    fn a_streamed_body_states_no_length() {
        let body = Streamed::new(
            producing(FRAMES),
            Coding::Gzip,
            Levels::default(),
            LatencyMode::Interactive,
        );

        assert_eq!(body.size_hint().exact(), None);
        assert!(!body.is_end_stream());
    }

    /// An empty stream is still a well-formed member of its coding: a gzip
    /// stream with no data is a header and a trailer, not zero bytes.
    #[tokio::test]
    async fn a_stream_that_produced_nothing_is_still_a_valid_member_of_its_coding() {
        let produced = encoded(&[], LatencyMode::Interactive).await;

        assert!(!produced.is_empty(), "nothing at all was sent");
        assert_eq!(decoded(&produced).await, "");
    }
}
