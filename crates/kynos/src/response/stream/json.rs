//! Streamed JSON: newline-delimited, and RFC 7464 text sequences.

use crate::{
    http::Response,
    response::{IntoResponse, Responses},
    schema::{Registry, Schema},
};

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
        todo!()
    }
}

impl<S> Responses for JsonLines<S>
where
    S: futures_core::Stream,
    S::Item: Schema,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
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
        todo!()
    }
}

impl<S> Responses for JsonSeq<S>
where
    S: futures_core::Stream,
    S::Item: Schema,
{
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
