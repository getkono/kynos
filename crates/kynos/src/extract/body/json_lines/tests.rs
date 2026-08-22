//! The streamed JSON decoder: what it recovers, and what it answers when it
//! cannot.
//!
//! `docs/testing.md` calls this a parser — an open input space — so what it
//! owes is an independently constructed oracle and one case per error variant
//! counted against the source. Two of the spaces here close, and the document
//! prefers a sweep to a draw where they do: the frame boundaries of a fixed
//! body, and the arrangements of the framing around a fixed set of records.
//!
//! No socket is involved, and none is owed: the Runtime I/O row wants one for a
//! socket, a timer, a task or a signal, and a decoder has none. Multi-frame
//! arrival is reachable in-crate because `Body::from_stream` is `pub(crate)`
//! and gated on `openapi32`, which is the gate this module is already under.
//! `tests/sse.rs` hand-writes a chunk stream for the same reason.

use std::{
    io,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use bytes::Bytes;
use futures_core::Stream;
use kynos_openapi::RefOr;

use super::{JsonLines, JsonSeq, records::Records};
use crate::{
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{Request, body::Body, header},
    router::operation::OperationCx,
    schema::registry::Registry,
};

const NDJSON: &str = "application/x-ndjson";
const JSON_SEQ: &str = "application/json-seq";
const RECORD_SEPARATOR: u8 = 0x1e;

/// One frame of a body, or the transport failing to deliver one.
type Frame = Result<Bytes, io::Error>;

#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
struct Reading {
    at: u32,
}

/// A body arriving as exactly the frames it was given.
///
/// Hand-written for the reason `tests/sse.rs` hand-writes one: the UI
/// suite's snapshots embed rustc's implementor lists, so a dev-dependency
/// on a stream library would rework dozens of unrelated `.stderr` files.
struct Frames {
    remaining: std::vec::IntoIter<Frame>,
    /// Whether the stream pends rather than ending once the frames run
    /// out, which is what an unfinished request body does.
    then_pending: bool,
}

impl Stream for Frames {
    type Item = Frame;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.remaining.next() {
            Some(frame) => Poll::Ready(Some(frame)),
            None if self.then_pending => Poll::Pending,
            None => Poll::Ready(None),
        }
    }
}

/// A request carrying `frames`, under `content_type` when there is one.
fn request(content_type: Option<&str>, frames: Vec<Frame>, then_pending: bool) -> Request {
    let mut builder = http::Request::builder().method("POST").uri("/");
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    builder
        .body(Body::from_stream(Frames {
            remaining: frames.into_iter(),
            then_pending,
        }))
        .expect("a well-formed request")
}

/// The whole body in one frame.
fn one_frame(bytes: &[u8]) -> Vec<Frame> {
    vec![Ok(Bytes::copy_from_slice(bytes))]
}

/// Extracts a newline-delimited body, which only the content type can
/// refuse.
async fn ndjson(frames: Vec<Frame>) -> Records<Reading> {
    JsonLines::<Records<Reading>>::from_request(request(Some(NDJSON), frames, false), &())
        .await
        .expect("`application/x-ndjson` is the media type this body extracts")
        .items
}

/// Extracts an RFC 7464 sequence body.
async fn json_seq(frames: Vec<Frame>) -> Records<Reading> {
    JsonSeq::<Records<Reading>>::from_request(request(Some(JSON_SEQ), frames, false), &())
        .await
        .expect("`application/json-seq` is the media type this body extracts")
        .items
}

/// Three records, with the values recorded as the bytes are assembled.
///
/// The oracle never consults the decoder, which is the whole of the parser
/// rule: one derived from the parser agrees with it wherever both are
/// wrong.
fn three_lines() -> (Vec<u8>, Vec<Reading>) {
    let mut bytes = Vec::new();
    let mut expected = Vec::new();

    for at in [1_u32, 2, 3] {
        bytes.extend_from_slice(format!("{{\"at\":{at}}}").as_bytes());
        bytes.push(b'\n');
        expected.push(Reading { at });
    }

    (bytes, expected)
}

/// Every way a fixed body can be split into two frames, including the two
/// degenerate ones where a frame is empty.
///
/// A record spanning a frame boundary is the failure a whole-body test
/// cannot see, and the space of boundaries closes, so it is swept rather
/// than sampled.
#[tokio::test]
async fn a_record_survives_a_frame_boundary_at_every_offset() {
    let (bytes, expected) = three_lines();

    for split in 0..=bytes.len() {
        let frames = vec![
            Ok(Bytes::copy_from_slice(&bytes[..split])),
            Ok(Bytes::copy_from_slice(&bytes[split..])),
        ];

        let read = ndjson(frames)
            .await
            .read_all()
            .await
            .unwrap_or_else(|error| panic!("split at {split}: {error}"));

        assert_eq!(read, expected, "split at {split}");
    }
}

/// One frame per byte, which is the same body arriving as slowly as it can.
#[tokio::test]
async fn a_record_survives_arriving_one_byte_at_a_time() {
    let (bytes, expected) = three_lines();
    let frames = bytes
        .iter()
        .map(|byte| Ok(Bytes::copy_from_slice(&[*byte])))
        .collect();

    assert_eq!(
        ndjson(frames)
            .await
            .read_all()
            .await
            .expect("three records"),
        expected
    );
}

/// Every arrangement of the framing a writer may legitimately produce,
/// against the same three records.
///
/// Twenty-four arrangements: a leading blank line or not, an interior blank
/// line or not, `\r\n` line endings or `\n`, and a last record ending with
/// nothing, one newline or two. None of them changes what the body carries,
/// which is the claim.
#[tokio::test]
async fn every_arrangement_of_the_framing_carries_the_same_records() {
    for leading_blank in [false, true] {
        for interior_blank in [false, true] {
            for carriage_return in [false, true] {
                for trailing in [0_usize, 1, 2] {
                    let ending: &[u8] = if carriage_return { b"\r\n" } else { b"\n" };
                    let mut bytes = Vec::new();
                    let mut expected = Vec::new();

                    if leading_blank {
                        bytes.extend_from_slice(ending);
                    }

                    for at in [1_u32, 2, 3] {
                        if interior_blank && at == 3 {
                            bytes.extend_from_slice(ending);
                        }
                        bytes.extend_from_slice(format!("{{\"at\":{at}}}").as_bytes());
                        if at < 3 {
                            bytes.extend_from_slice(ending);
                        }
                        expected.push(Reading { at });
                    }

                    for _ in 0..trailing {
                        bytes.extend_from_slice(ending);
                    }

                    let arrangement = format!(
                        "leading {leading_blank}, interior {interior_blank}, crlf \
                         {carriage_return}, trailing {trailing}"
                    );
                    let read = ndjson(one_frame(&bytes))
                        .await
                        .read_all()
                        .await
                        .unwrap_or_else(|error| panic!("{arrangement}: {error}"));

                    assert_eq!(read, expected, "{arrangement}");
                }
            }
        }
    }
}

/// The same sweep for RFC 7464, where the separator leads rather than
/// trails — and where one record may hold newlines, which is what the
/// framing is for.
#[tokio::test]
async fn every_arrangement_of_a_sequence_carries_the_same_records() {
    for leading_blank in [false, true] {
        for interior_blank in [false, true] {
            for pretty in [false, true] {
                for trailing in [0_usize, 1, 2] {
                    let mut bytes = Vec::new();
                    let mut expected = Vec::new();

                    if leading_blank {
                        bytes.push(RECORD_SEPARATOR);
                    }

                    for at in [1_u32, 2, 3] {
                        if interior_blank && at == 3 {
                            bytes.push(RECORD_SEPARATOR);
                        }
                        bytes.push(RECORD_SEPARATOR);
                        let record = if pretty {
                            format!("{{\n  \"at\": {at}\n}}")
                        } else {
                            format!("{{\"at\":{at}}}")
                        };
                        bytes.extend_from_slice(record.as_bytes());
                        bytes.push(b'\n');
                        expected.push(Reading { at });
                    }

                    for _ in 0..trailing {
                        bytes.push(RECORD_SEPARATOR);
                    }

                    let arrangement = format!(
                        "leading {leading_blank}, interior {interior_blank}, pretty {pretty}, \
                         trailing {trailing}"
                    );
                    let read = json_seq(one_frame(&bytes))
                        .await
                        .read_all()
                        .await
                        .unwrap_or_else(|error| panic!("{arrangement}: {error}"));

                    assert_eq!(read, expected, "{arrangement}");
                }
            }
        }
    }
}

/// The lag the separator's position forces, and that `JsonLines` does not
/// have.
///
/// With the body still open, a sequence has delivered every record but the
/// last: nothing has said the last one is over. The same bytes framed as
/// lines deliver all three, because there the delimiter trails.
#[tokio::test]
async fn a_sequence_holds_its_last_record_until_the_body_ends() {
    let mut sequence = JsonSeq::<Records<Reading>>::from_request(
        request(
            Some(JSON_SEQ),
            one_frame(b"\x1e{\"at\":1}\n\x1e{\"at\":2}\n"),
            true,
        ),
        &(),
    )
    .await
    .expect("the sequence media type")
    .items;

    let mut context = Context::from_waker(Waker::noop());

    assert!(matches!(
        Pin::new(&mut sequence).poll_next(&mut context),
        Poll::Ready(Some(Ok(Reading { at: 1 })))
    ));
    assert!(
        matches!(
            Pin::new(&mut sequence).poll_next(&mut context),
            Poll::Pending
        ),
        "the second record is only known complete once a separator or the body's end follows"
    );

    let mut lines = JsonLines::<Records<Reading>>::from_request(
        request(Some(NDJSON), one_frame(b"{\"at\":1}\n{\"at\":2}\n"), true),
        &(),
    )
    .await
    .expect("the lines media type")
    .items;

    assert!(matches!(
        Pin::new(&mut lines).poll_next(&mut context),
        Poll::Ready(Some(Ok(Reading { at: 1 })))
    ));
    assert!(
        matches!(
            Pin::new(&mut lines).poll_next(&mut context),
            Poll::Ready(Some(Ok(Reading { at: 2 })))
        ),
        "a newline ends a record, so the last one does not wait for what follows it"
    );
}

/// A body with no bytes carries no records, and that is not a failure.
#[tokio::test]
async fn an_empty_body_carries_no_records() {
    assert_eq!(
        ndjson(one_frame(b""))
            .await
            .read_all()
            .await
            .expect("no records"),
        Vec::new()
    );
    assert_eq!(
        json_seq(one_frame(b""))
            .await
            .read_all()
            .await
            .expect("no records"),
        Vec::new()
    );
}

/// The one parameter a codec accepts, because it names the encoding Kynos
/// already assumes.
#[tokio::test]
async fn a_utf_8_charset_is_accepted() {
    let records = JsonLines::<Records<Reading>>::from_request(
        request(
            Some("application/x-ndjson; charset=utf-8"),
            one_frame(b"{\"at\":1}\n"),
            false,
        ),
        &(),
    )
    .await
    .expect("`charset=utf-8` names the encoding this decoder reads")
    .items;

    assert_eq!(
        records.read_all().await.expect("one record"),
        vec![Reading { at: 1 }]
    );
}

/// A streamed body describes one *item*, and says nothing about the whole.
///
/// 3.2 permits `schema` beside `itemSchema` and says it is unlikely to
/// help; an array `schema` here would also contradict what the response
/// half emits for the same media type.
#[test]
fn a_streamed_body_describes_its_item_and_not_the_whole() {
    for (media_type, body) in [
        (
            "application/x-ndjson",
            JsonLines::<Records<u64>>::request_body(&mut Registry::default()),
        ),
        (
            "application/json-seq",
            JsonSeq::<Records<u64>>::request_body(&mut Registry::default()),
        ),
    ] {
        let content = body
            .content
            .get(media_type)
            .unwrap_or_else(|| panic!("the body is keyed by {media_type}"));

        assert!(
            content.item_schema.is_some(),
            "{media_type} describes each streamed value"
        );
        assert_eq!(
            content.schema, None,
            "{media_type} makes no claim about the body as a whole"
        );
        assert_eq!(body.required, Some(true));
    }
}

/// `Option` makes the body optional and adds nothing else, exactly as it
/// does for a whole-body codec.
#[test]
fn an_optional_streamed_body_is_not_required() {
    let mut registry = Registry::default();
    let mut operation = OperationCx::new(&mut registry);
    <Option<JsonLines<Records<u64>>> as Describe>::describe(&mut operation);

    let Some(RefOr::Item(body)) = operation.finish().request_body else {
        panic!("an optional body is still a body");
    };

    assert_eq!(body.required, Some(false));
    assert!(body.content.contains_key("application/x-ndjson"));
}
