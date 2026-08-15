//! Streamed JSON: newline-delimited, and RFC 7464 text sequences.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

/// The error any body reports, whatever the stream's own error type was.
type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The record separator RFC 7464 puts before each JSON text.
///
/// It cannot occur inside a JSON text, which is the whole reason the framing
/// exists: a value holding a newline stays one record.
const RECORD_SEPARATOR: &str = "\u{1e}";

/// A newline-delimited JSON response (`application/x-ndjson`).
///
/// Requires both `json` and `openapi32`; the latter supplies the `itemSchema`
/// needed to describe each streamed value.
///
/// ```no_run
/// # #[cfg(all(feature = "json", feature = "openapi32"))]
/// # {
/// use kynos::response::stream::json::JsonLines;
///
/// fn lines<S>(items: S) -> JsonLines<S> {
///     JsonLines { items }
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct JsonLines<S> {
    /// The stream of items.
    pub items: S,
}

/// An RFC 7464 JSON text sequence response (`application/json-seq`).
///
/// Requires both `json` and `openapi32`; the latter supplies the `itemSchema`
/// needed to describe each streamed value.
///
/// ```no_run
/// # #[cfg(all(feature = "json", feature = "openapi32"))]
/// # {
/// use kynos::response::stream::json::JsonSeq;
///
/// fn sequence<S>(items: S) -> JsonSeq<S> {
///     JsonSeq { items }
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct JsonSeq<S> {
    /// The stream of items.
    pub items: S,
}

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
        let mut response = Response::new(Body::from_stream(Framed::new(self.items, "", "\n")));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-ndjson"),
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
                "application/x-ndjson",
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
            RECORD_SEPARATOR,
            "\n",
        )));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json-seq"),
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
                "application/json-seq",
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
    prefix: &'static str,
    suffix: &'static str,
}

impl<S> Framed<S> {
    fn new(items: S, prefix: &'static str, suffix: &'static str) -> Self {
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
    prefix: &str,
    suffix: &str,
) -> Result<bytes::Bytes, BoxError> {
    let mut record = Vec::from(prefix.as_bytes());
    serde_json::to_writer(&mut record, item)?;
    record.extend_from_slice(suffix.as_bytes());

    Ok(bytes::Bytes::from(record))
}
