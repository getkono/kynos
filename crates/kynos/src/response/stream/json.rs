//! Streamed JSON: newline-delimited, and RFC 7464 text sequences.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    extract::body::json_lines::{LINES_MEDIA_TYPE, SEQUENCE_MEDIA_TYPE, records::RECORD_SEPARATOR},
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

/// The error any body reports, whatever the stream's own error type was.
type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Nothing in front of an NDJSON record.
const NO_PREFIX: &[u8] = b"";

/// The newline that ends an NDJSON record, and that RFC 7464 admits after a
/// JSON text in a sequence.
const NEWLINE: &[u8] = b"\n";

/// RFC 7464's separator, written from the one byte the decoder scans for.
const SEQUENCE_PREFIX: &[u8] = &[RECORD_SEPARATOR];

use crate::extract::body::json_lines::{JsonLines, JsonSeq};

/// Streams each item as one JSON value followed by a newline.
///
/// The response is committed before every item is available. If serializing a
/// later item fails, the stream terminates; it cannot replace the already-sent
/// status with a problem response.
impl<S> IntoResponse for JsonLines<S>
where
    S: futures_core::Stream + Send + 'static,
    S::Item: serde::Serialize,
{
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::from_stream(Framed::new(
            self.items, NO_PREFIX, NEWLINE,
        )));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(LINES_MEDIA_TYPE),
        );
        response
    }
}

impl<S> Responses for JsonLines<S>
where
    S: futures_core::Stream,
    S::Item: Schema,
{
    // `itemSchema` rather than `schema`: what a consumer reads one line at a
    // time is one item, and 3.2 added the field so that saying so is possible.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                LINES_MEDIA_TYPE,
                kynos_openapi::MediaType::sequential(registry.resolve::<S::Item>()),
            ),
        )
    }
}

/// Streams each item as an RFC 7464 JSON text sequence record.
///
/// The response is committed before every item is available. If serializing a
/// later item fails, the stream terminates; it cannot replace the already-sent
/// status with a problem response.
impl<S> IntoResponse for JsonSeq<S>
where
    S: futures_core::Stream + Send + 'static,
    S::Item: serde::Serialize,
{
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::from_stream(Framed::new(
            self.items,
            SEQUENCE_PREFIX,
            NEWLINE,
        )));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(SEQUENCE_MEDIA_TYPE),
        );
        response
    }
}

impl<S> Responses for JsonSeq<S>
where
    S: futures_core::Stream,
    S::Item: Schema,
{
    // The framing differs from JSON Lines and the described item does not: both
    // repeat one JSON value, which is what a sequential media type is.
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                SEQUENCE_MEDIA_TYPE,
                kynos_openapi::MediaType::sequential(registry.resolve::<S::Item>()),
            ),
        )
    }
}

/// The items, framed as the bytes of one record each.
///
/// Both framings here are one JSON value between a prefix and a suffix, so one
/// adapter carries both rather than two that differ in two string constants.
///
/// The stream is held boxed so that it can be polled without a projection:
/// `Pin<Box<S>>` is `Unpin` whatever `S` is, and `unsafe` is forbidden here.
struct Framed<S> {
    items: Pin<Box<S>>,
    prefix: &'static [u8],
    suffix: &'static [u8],
}

impl<S> Framed<S> {
    fn new(items: S, prefix: &'static [u8], suffix: &'static [u8]) -> Self {
        Self {
            items: Box::pin(items),
            prefix,
            suffix,
        }
    }
}

impl<S> futures_core::Stream for Framed<S>
where
    S: futures_core::Stream,
    S::Item: serde::Serialize,
{
    type Item = Result<bytes::Bytes, BoxError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let framed = self.get_mut();
        let (prefix, suffix) = (framed.prefix, framed.suffix);

        match framed.items.as_mut().poll_next(context) {
            // A failure here has no status left to spend -- the 200 went out
            // with the first record -- so the body ends rather than lies.
            Poll::Ready(Some(item)) => Poll::Ready(Some(encode(&item, prefix, suffix))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Serializes one item into its framed record.
fn encode<T: serde::Serialize>(
    item: &T,
    prefix: &[u8],
    suffix: &[u8],
) -> Result<bytes::Bytes, BoxError> {
    let mut record = Vec::from(prefix);
    serde_json::to_writer(&mut record, item)?;
    record.extend_from_slice(suffix);

    Ok(bytes::Bytes::from(record))
}
