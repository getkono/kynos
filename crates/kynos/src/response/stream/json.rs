//! Streamed JSON: newline-delimited, and RFC 7464 text sequences.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    extract::body::json_lines::{LINES_MEDIA_TYPE, SEQUENCE_MEDIA_TYPE, records::RECORD_SEPARATOR},
    http::{
        HeaderValue, Response,
        body::{Body, BoxError},
    },
    response::{
        IntoResponse, Responses,
        codec::{ok, with_content_type},
        stream::records::RecordBuffer,
    },
    schema::{Schema, registry::Registry},
};

/// Nothing in front of an NDJSON record.
const NO_PREFIX: &[u8] = b"";

/// The newline that ends an NDJSON record, and that RFC 7464 admits after a
/// JSON text in a sequence.
const NEWLINE: &[u8] = b"\n";

/// RFC 7464's separator, written from the one byte the decoder scans for.
const SEQUENCE_PREFIX: &[u8] = &[RECORD_SEPARATOR];

use crate::extract::body::json_lines::{JsonLines, JsonSeq};

/// Streams each item as one JSON value followed by a newline.
impl<S> IntoResponse for JsonLines<S>
where
    S: futures_core::Stream + Send + 'static,
    S::Item: serde::Serialize,
{
    fn into_response(self) -> Response {
        with_content_type(
            Body::from_stream(Framed::new(self.items, NO_PREFIX, NEWLINE)),
            HeaderValue::from_static(LINES_MEDIA_TYPE),
        )
    }
}

impl<S> Responses for JsonLines<S>
where
    S: futures_core::Stream,
    S::Item: Schema,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        ok(
            LINES_MEDIA_TYPE,
            kynos_openapi::MediaType::sequential(registry.resolve::<S::Item>()),
        )
    }
}

/// Streams each item as an RFC 7464 JSON text sequence record.
impl<S> IntoResponse for JsonSeq<S>
where
    S: futures_core::Stream + Send + 'static,
    S::Item: serde::Serialize,
{
    fn into_response(self) -> Response {
        with_content_type(
            Body::from_stream(Framed::new(self.items, SEQUENCE_PREFIX, NEWLINE)),
            HeaderValue::from_static(SEQUENCE_MEDIA_TYPE),
        )
    }
}

impl<S> Responses for JsonSeq<S>
where
    S: futures_core::Stream,
    S::Item: Schema,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        ok(
            SEQUENCE_MEDIA_TYPE,
            kynos_openapi::MediaType::sequential(registry.resolve::<S::Item>()),
        )
    }
}

/// The items, each framed as one JSON value between a prefix and a suffix.
///
/// Boxed so `Pin<Box<S>>` is `Unpin` and polls without an `unsafe` projection.
struct Framed<S> {
    items: Pin<Box<S>>,
    prefix: &'static [u8],
    suffix: &'static [u8],
    records: RecordBuffer,
}

impl<S> Framed<S> {
    fn new(items: S, prefix: &'static [u8], suffix: &'static [u8]) -> Self {
        Self {
            items: Box::pin(items),
            prefix,
            suffix,
            records: RecordBuffer::new(),
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
            Poll::Ready(Some(item)) => {
                Poll::Ready(Some(encode(&mut framed.records, &item, prefix, suffix)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Serializes one item into its framed record, in `records`' block.
fn encode<T: serde::Serialize>(
    records: &mut RecordBuffer,
    item: &T,
    prefix: &[u8],
    suffix: &[u8],
) -> Result<bytes::Bytes, BoxError> {
    records.record(|record| {
        record.extend_from_slice(prefix);
        serde_json::to_writer(Appending(record), item)?;
        record.extend_from_slice(suffix);
        Ok(())
    })
}

/// A block written as a `Vec<u8>` is: every write appends whole.
///
/// Rather than `BufMut::writer`, whose `write` may stop short, so that each of
/// the many small `write_all`s `serde_json` makes is one append rather than a
/// loop around one.
struct Appending<'a>(&'a mut bytes::BytesMut);

impl std::io::Write for Appending<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
