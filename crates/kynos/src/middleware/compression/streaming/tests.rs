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
