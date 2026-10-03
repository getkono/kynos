//! The bytes the two streamed JSON responses put on the wire.
//!
//! Each expectation is written out as literal bytes rather than derived from
//! [`encode`](super::encode), since an oracle built from the writer agrees with
//! it wherever both are wrong.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_core::Stream;
use http_body_util::BodyExt;

use crate::{
    extract::{
        FromRequest,
        body::json_lines::{JsonLines, JsonSeq, records::Records},
    },
    http::{Request, Response, header},
    response::IntoResponse,
};

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Reading {
    at: u32,
}

/// An item whose `Serialize` always fails, as a map with a non-string key
/// does at run time.
struct Unserializable;

impl serde::Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("this item cannot be written"))
    }
}

/// Either kind of item, so one stream can hold a good record and a bad one.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum Item {
    Good(Reading),
    Bad(Unserializable),
}

/// A stream yielding exactly the items it was given, then ending.
///
/// Hand-written for the reason `json_lines/tests.rs` hand-writes its frames:
/// a stream library as a dev-dependency would rework the UI snapshots.
struct Items<T>(std::vec::IntoIter<T>);

impl<T: Unpin> Stream for Items<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<T>> {
        Poll::Ready(self.0.next())
    }
}

fn items<T>(items: Vec<T>) -> Items<T> {
    Items(items.into_iter())
}

fn readings() -> Vec<Reading> {
    vec![Reading { at: 1 }, Reading { at: 2 }]
}

/// Every data frame the body yields, then whether it ended cleanly.
///
/// Frame by frame rather than collected, because one frame per record is part
/// of what is asserted: a record is never split or merged with its neighbour.
async fn frames(response: Response) -> (Vec<Bytes>, Result<(), String>) {
    let mut body = response.into_body();
    let mut frames = Vec::new();

    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => frames.push(frame.into_data().expect("a data frame")),
            Err(error) => return (frames, Err(error.to_string())),
        }
    }

    (frames, Ok(()))
}

fn content_type(response: &Response) -> &str {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("a content type")
        .to_str()
        .expect("a printable content type")
}

#[tokio::test]
async fn json_lines_writes_one_newline_terminated_value_per_item() {
    let response = JsonLines {
        items: items(readings()),
    }
    .into_response();
    assert_eq!(content_type(&response), "application/x-ndjson");

    let (frames, ended) = frames(response).await;

    assert_eq!(
        frames,
        [&b"{\"at\":1}\n"[..], &b"{\"at\":2}\n"[..]],
        "one value and one newline per item, with nothing in front"
    );
    assert_eq!(ended, Ok(()));
}

/// RFC 7464 section 2: each JSON text is preceded by RS (0x1E) and followed by
/// a line feed.
#[tokio::test]
async fn json_seq_prefixes_each_record_with_the_record_separator() {
    let response = JsonSeq {
        items: items(readings()),
    }
    .into_response();
    assert_eq!(content_type(&response), "application/json-seq");

    let (frames, ended) = frames(response).await;

    assert_eq!(frames, [&b"\x1e{\"at\":1}\n"[..], &b"\x1e{\"at\":2}\n"[..]],);
    assert_eq!(ended, Ok(()));
}

/// The status went out with the first record, so a later failure can only
/// end the body: the records before it arrive whole, and the failing record is
/// reported as an error rather than skipped over to the one after it.
#[tokio::test]
async fn a_record_that_fails_to_serialize_ends_the_body_in_error() {
    let response = JsonLines {
        items: items(vec![
            Item::Good(Reading { at: 1 }),
            Item::Bad(Unserializable),
            Item::Good(Reading { at: 3 }),
        ]),
    }
    .into_response();
    assert_eq!(response.status(), crate::http::StatusCode::OK);

    let (frames, ended) = frames(response).await;

    assert_eq!(frames, [&b"{\"at\":1}\n"[..]]);
    assert_eq!(ended, Err("this item cannot be written".to_owned()));
}

/// The two halves of the codec agree: a response body read back through the
/// extractor yields the items that were written, for both framings.
#[tokio::test]
async fn what_json_lines_writes_the_json_lines_extractor_reads_back() {
    let carrying = |response: Response| -> Request {
        let content_type = response.headers()[header::CONTENT_TYPE].clone();
        http::Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, content_type)
            .body(response.into_body())
            .expect("a well-formed request")
    };

    let lines = JsonLines {
        items: items(readings()),
    }
    .into_response();
    let read: Vec<Reading> = JsonLines::<Records<Reading>>::from_request(carrying(lines), &())
        .await
        .expect("the response's own media type")
        .items
        .read_all()
        .await
        .expect("every record decodes");
    assert_eq!(read, readings());

    let sequence = JsonSeq {
        items: items(readings()),
    }
    .into_response();
    let read: Vec<Reading> = JsonSeq::<Records<Reading>>::from_request(carrying(sequence), &())
        .await
        .expect("the response's own media type")
        .items
        .read_all()
        .await
        .expect("every record decodes");
    assert_eq!(read, readings());
}
