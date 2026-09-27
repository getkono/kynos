use std::collections::VecDeque;

use async_compression::tokio::bufread::GzipDecoder;
use http_body_util::BodyExt as _;
use tokio::io::AsyncReadExt as _;

use super::{
    Bytes, Coding, Context, Frame, HttpBody, LatencyMode, Levels, Pin, Poll, SizeHint, Streamed,
    accepted, io,
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

/// A body of unknown length that yields one data frame, then fails.
struct FailsAfterOne(u8);

impl HttpBody for FailsAfterOne {
    type Data = Bytes;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        this.0 += 1;

        match this.0 {
            1 => Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"first frame\n"))))),
            2 => Poll::Ready(Some(Err("the producer failed part-way".into()))),
            _ => Poll::Ready(None),
        }
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

/// `FailsAfterOne`, encoded.
fn failing() -> Streamed {
    Streamed::new(
        crate::http::body::Body::from_body(FailsAfterOne(0)),
        Coding::Gzip,
        Levels::default(),
        LatencyMode::Interactive,
    )
}

/// `FRAMES`, encoded.
fn finishing() -> Streamed {
    Streamed::new(
        producing(FRAMES),
        Coding::Gzip,
        Levels::default(),
        LatencyMode::Interactive,
    )
}

/// Wraps `body` the way the dispatcher does, reads it to its end or its first
/// error, drops it as a driver would, and hands back what it reported.
async fn delivery_of(body: Streamed) -> Vec<crate::http::body::Delivery> {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&seen);

    let _outcome = crate::http::body::Body::from_body(body)
        .watching(move |delivery| sink.lock().expect("an unpoisoned lock").push(delivery))
        .collect()
        .await;

    seen.lock().expect("an unpoisoned lock").clone()
}

/// A body that failed did not end, and says so from then on.
///
/// `Watched` decides `Complete` against `Interrupted` by asking exactly this
/// when it is dropped, so an error that left the answer `true` would report a
/// broken response as delivered.
#[tokio::test]
async fn a_failed_stream_does_not_report_its_end() {
    let mut body = failing();

    while Pin::new(&mut body)
        .frame()
        .await
        .expect("the failure is yielded before the body ends")
        .is_ok()
    {}

    assert!(
        !body.is_end_stream(),
        "a stream whose producer failed reports that it ended"
    );
    assert!(
        Pin::new(&mut body).frame().await.is_none(),
        "a failed stream yielded more after its error"
    );
    assert!(!body.is_end_stream());
}

/// The observable half: the failure reaches an observer as an interruption.
#[tokio::test]
async fn a_failed_stream_is_reported_as_interrupted() {
    assert_eq!(
        delivery_of(failing()).await,
        vec![crate::http::body::Delivery::Interrupted]
    );
}

/// The pass control: a stream that finished its coding reports its end, so
/// the two cases above are about the failure rather than about every stream.
#[tokio::test]
async fn a_finished_stream_reports_its_end() {
    let mut body = finishing();

    while let Some(frame) = Pin::new(&mut body).frame().await {
        frame.expect("an encoded frame");
    }

    assert!(body.is_end_stream());
    assert_eq!(
        delivery_of(finishing()).await,
        vec![crate::http::body::Delivery::Complete]
    );
}

/// An encoder that took nothing of what it was given is broken, not busy:
/// taking it at its word would poll it again forever.
#[test]
fn a_write_that_took_nothing_is_a_write_zero_failure() {
    assert_eq!(
        accepted(Ok(0)).map_err(|error| error.kind()),
        Err(io::ErrorKind::WriteZero)
    );
}

/// The control: a write that took something is that many bytes taken, and a
/// write that failed is its own failure rather than `WriteZero`.
#[test]
fn a_write_that_took_bytes_or_failed_is_reported_as_it_was() {
    assert_eq!(accepted(Ok(3)).map_err(|error| error.kind()), Ok(3));
    assert_eq!(
        accepted(Err(io::Error::from(io::ErrorKind::BrokenPipe))).map_err(|error| error.kind()),
        Err(io::ErrorKind::BrokenPipe)
    );
}
