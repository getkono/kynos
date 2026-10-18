//! The wire shape each codec writes.
//!
//! One exact-bytes case and one media type per codec, and nothing more. A codec
//! is a value type in `docs/testing.md`'s allocation, which owes exactly that
//! and explicitly does not owe per-field tests or a hand-written round-trip:
//! the round-trip is the extracting half's, and a hand-written one here would
//! compare `encode` with `decode` and pass wherever both were wrong. The one
//! exception is JSON's writer, whose stack-to-heap spill is internal logic of
//! its own and is held at its boundaries.
//!
//! What an exact-bytes case buys is the thing a round-trip cannot see. A
//! misspelled field name round-trips perfectly — nothing sets
//! `deny_unknown_fields`, so it is written back unchanged and compares equal —
//! while the real field stays absent from end to end.

use crate::{
    http::{Response, header},
    response::IntoResponse,
};

/// The body a response carries, read to its end.
async fn body(response: Response) -> bytes::Bytes {
    use http_body_util::BodyExt;

    response
        .into_body()
        .collect()
        .await
        .expect("a readable body")
        .to_bytes()
}

/// The `Content-Type` a response stated.
fn media_type(response: &Response) -> &str {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("a stated media type")
        .to_str()
        .expect("a printable media type")
}

/// A value every codec that takes one can write.
#[cfg(any(feature = "json", feature = "form"))]
#[derive(serde::Serialize)]
struct Point {
    x: i32,
    y: i32,
}

#[cfg(feature = "json")]
#[tokio::test]
async fn json_writes_the_document_and_states_its_type() {
    use crate::extract::body::json::Json;

    let response = Json(Point { x: 1, y: -2 }).into_response();

    assert_eq!(media_type(&response), "application/json");
    assert_eq!(&body(response).await[..], br#"{"x":1,"y":-2}"#);
}

/// The writer moves a body from the stack to the heap once it outgrows 1 KiB:
/// a body filling it exactly, one octet past it, and one spilled far enough
/// that the heap grows again are each written whole and in order.
#[cfg(feature = "json")]
#[tokio::test]
async fn json_writes_a_body_whole_on_either_side_of_the_stack_buffer() {
    use crate::extract::body::json::Json;

    for octets in [1024, 1025, 5000] {
        // A string is written as its characters between two quotes.
        let text: String = ('a'..='z').cycle().take(octets - 2).collect();
        let response = Json(&text).into_response();

        assert_eq!(body(response).await, format!("\"{text}\"").as_bytes());
    }
}

#[cfg(feature = "form")]
#[tokio::test]
async fn a_form_writes_the_pairs_and_states_its_type() {
    use crate::extract::body::form::Form;

    let response = Form(Point { x: 1, y: -2 }).into_response();

    assert_eq!(media_type(&response), "application/x-www-form-urlencoded");
    assert_eq!(&body(response).await[..], b"x=1&y=-2");
}

/// The one shape the scalar case cannot show: an array property is written as
/// the description's default `form` style with `explode` states it, one pair
/// per item under the property's name, rather than refused.
#[cfg(feature = "form")]
#[tokio::test]
async fn a_form_writes_an_array_as_one_pair_per_item() {
    use crate::extract::body::form::Form;

    #[derive(serde::Serialize)]
    struct Tags {
        tag: Vec<&'static str>,
    }

    let response = Form(Tags {
        tag: vec!["a", "b"],
    })
    .into_response();

    assert_eq!(&body(response).await[..], b"tag=a&tag=b");
}

/// A value the form encoder refuses -- here a bare number, which has no name
/// to pair it with -- answers a 500 problem rather than a 200 with a partial
/// or empty body: it is encoded in full before the status is committed.
#[cfg(feature = "form")]
#[tokio::test]
async fn a_form_that_cannot_be_encoded_answers_500() {
    use crate::extract::body::form::Form;

    let response = Form(42_u32).into_response();

    assert_eq!(
        response.status(),
        crate::http::StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(media_type(&response), "application/problem+json");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body(response).await)
            .expect("a problem document"),
        serde_json::json!({
            "type": "about:blank",
            "title": "Internal Server Error",
            "status": 500,
            "detail": "the response body could not be encoded as a form",
        })
    );
}

#[tokio::test]
async fn text_writes_the_string_and_states_its_charset() {
    use crate::extract::body::text::Text;

    let response = Text("héllo".to_owned()).into_response();

    // RFC 6657 removed `text/plain`'s US-ASCII default, and a Rust `String` is
    // UTF-8, so the charset is stated rather than assumed.
    assert_eq!(media_type(&response), "text/plain; charset=utf-8");
    assert_eq!(&body(response).await[..], "héllo".as_bytes());
}

#[tokio::test]
async fn binary_writes_its_bytes_verbatim_under_the_type_it_was_given() {
    use crate::{extract::body::binary::Binary, http::media::OctetStream};

    let response = Binary::<OctetStream>::new(&[0x00, 0xff, 0x10][..]).into_response();

    assert_eq!(media_type(&response), "application/octet-stream");
    assert_eq!(&body(response).await[..], &[0x00, 0xff, 0x10]);
}

#[cfg(feature = "protobuf")]
#[tokio::test]
async fn protobuf_writes_the_encoded_message_and_states_its_type() {
    use crate::extract::body::protobuf::Protobuf;

    #[derive(Clone, PartialEq, prost::Message)]
    struct Tagged {
        #[prost(int32, tag = "1")]
        value: i32,
    }

    let response = Protobuf(Tagged { value: 7 }).into_response();

    assert_eq!(media_type(&response), "application/protobuf");
    // Field 1, varint wire type, value 7 — the whole message.
    assert_eq!(&body(response).await[..], &[0x08, 0x07]);
}

#[cfg(feature = "multipart")]
#[tokio::test]
async fn multipart_states_the_boundary_the_body_is_framed_with() {
    use crate::extract::body::multipart::{MultipartForm, Part};
    use crate::response::codec::multipart::IntoMultipart;

    struct One;

    impl IntoMultipart for One {
        fn into_parts(self) -> Vec<Part> {
            vec![Part {
                name: "field".to_owned(),
                file_name: None,
                content_type: None,
                bytes: bytes::Bytes::from_static(b"value"),
            }]
        }
    }

    let response = MultipartForm(One).into_response();
    let stated = media_type(&response).to_owned();
    let boundary = stated
        .split_once("boundary=")
        .expect("a stated boundary")
        .1
        .to_owned();

    assert!(
        stated.starts_with("multipart/form-data; boundary="),
        "{stated}"
    );
    // The stated boundary is the one the body is framed with. A body framed
    // with a different delimiter than the header names is unreadable, and the
    // header is the only thing a reader has to go on.
    assert!(
        body(response)
            .await
            .starts_with(format!("--{boundary}\r\n").as_bytes())
    );
}

/// The codecs, counted against the cases above.
///
/// Under every feature, because that is the build where the whole set exists —
/// the same reason `pipeline.rs` gates its route-attribute counter.
#[cfg(all(
    feature = "json",
    feature = "form",
    feature = "protobuf",
    feature = "multipart"
))]
#[test]
fn every_codec_has_a_wire_case() {
    const SOURCE: &str = include_str!("mod.rs");
    /// The codecs a case above pins the bytes of, in declaration order.
    const WITNESSED: [&str; 6] = ["binary", "text", "form", "json", "multipart", "protobuf"];

    // Either visibility. What is counted is codecs, and whether a codec's
    // module is `pub` says only whether it declares an item of its own --
    // `multipart` declares two traits, the other five declare nothing and are
    // private. A codec still writes bytes either way, so keying this on `pub`
    // would have let the whole set go uncounted the day they were sealed.
    let declared = SOURCE
        .lines()
        .filter_map(|line| {
            line.strip_prefix("pub mod ")
                .or_else(|| line.strip_prefix("mod "))
        })
        .filter_map(|declaration| declaration.strip_suffix(';'))
        .filter(|name| *name != "tests")
        .collect::<Vec<_>>();

    assert_eq!(
        declared, WITNESSED,
        "`codec/` holds one set of codecs and another has a wire case; a codec added without \
         one is a codec whose bytes nothing pins"
    );
}
