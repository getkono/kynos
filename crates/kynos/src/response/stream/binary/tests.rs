use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_core::Stream;
use http_body_util::BodyExt;

use super::BinaryStream;
use crate::{
    http::{header, media::Png},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// A stream yielding exactly the chunks it was given, then ending.
///
/// Hand-written for the reason `stream/json/tests.rs` hand-writes its own: a
/// stream library as a dev-dependency would rework the UI snapshots.
struct Chunks(std::vec::IntoIter<Result<Bytes, std::io::Error>>);

impl Stream for Chunks {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.next())
    }
}

/// On the wire: the declared media type as `Content-Type`, and the chunks in
/// order, each its own frame.
#[tokio::test]
async fn a_binary_stream_sends_its_media_type_and_its_chunks() {
    let response = BinaryStream::<_, Png>::new(Chunks(
        vec![
            Ok(Bytes::from_static(b"\x89PNG")),
            Ok(Bytes::from_static(b"\r\n")),
        ]
        .into_iter(),
    ))
    .into_response();

    assert_eq!(response.status(), crate::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get_all(header::CONTENT_TYPE)
            .iter()
            .collect::<Vec<_>>(),
        ["image/png"]
    );

    let mut body = response.into_body();
    let mut frames = Vec::new();
    while let Some(frame) = body.frame().await {
        frames.push(
            frame
                .expect("a chunk that did not fail")
                .into_data()
                .expect("a data frame"),
        );
    }
    assert_eq!(
        frames,
        [Bytes::from_static(b"\x89PNG"), Bytes::from_static(b"\r\n")]
    );
}

/// In the description: one 200 under the declared media type whose `schema` is
/// the empty Schema Object and no `itemSchema` beside it, exactly as for
/// `Binary<M>`. The chunks are one representation, not a sequence of items, and
/// `itemSchema` is defined only for sequential media types.
#[test]
fn a_binary_stream_is_described_as_a_binary_body() {
    let described = BinaryStream::<Chunks, Png>::responses(&mut Registry::new());

    assert_eq!(
        serde_json::to_value(&described).expect("serializable responses"),
        serde_json::json!({
            "200": {
                "description": "OK",
                "content": { "image/png": { "schema": {} } },
            },
        })
    );
    assert_eq!(
        serde_json::to_value(&described).expect("serializable responses"),
        serde_json::to_value(crate::extract::body::binary::Binary::<Png>::responses(
            &mut Registry::new()
        ))
        .expect("serializable responses"),
        "a streamed binary body is described differently from a buffered one"
    );
}

/// A media type marker written as the docs write it, deriving nothing.
struct Bare;

/// The marker sits only in a `PhantomData`, so `Debug` is bounded on the stream
/// alone and prints it, leaving the marker out.
#[test]
fn a_marker_deriving_nothing_leaves_a_binary_stream_debug() {
    assert_eq!(
        format!("{:?}", BinaryStream::<_, Bare>::new(7_u8)),
        "BinaryStream { stream: 7, .. }"
    );
}
