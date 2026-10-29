//! Streamed binary content with a declared media type.

use crate::{
    http::{
        HeaderValue, Response,
        body::Body,
        media::{MediaType, WellFormed},
    },
    response::{
        IntoResponse, Responses,
        codec::{ok, unconstrained, with_content_type},
    },
    schema::registry::Registry,
};

/// A streamed binary response with a declared media type.
///
/// Described exactly as [`Binary<M>`](crate::extract::body::binary::Binary):
/// the chunks are one binary representation, not a sequence of items, so the
/// description is the same under OpenAPI 3.1 and 3.2.
///
/// ```no_run
/// use kynos::{http::media::OctetStream, response::stream::binary::BinaryStream};
///
/// fn download<S>(chunks: S) -> BinaryStream<S, OctetStream> {
///     BinaryStream::new(chunks)
/// }
/// ```
pub struct BinaryStream<S, M> {
    /// The stream producing byte chunks.
    pub stream: S,
    media_type: std::marker::PhantomData<M>,
}

// By hand, so it is bounded on `S` alone: a media type is a marker, and a
// derive would ask it to be `Debug` too.
impl<S: std::fmt::Debug, M> std::fmt::Debug for BinaryStream<S, M> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BinaryStream")
            .field("stream", &self.stream)
            .finish_non_exhaustive()
    }
}

impl<S, M> BinaryStream<S, M> {
    /// Creates a streamed response from byte chunks.
    #[must_use]
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            media_type: std::marker::PhantomData,
        }
    }
}

impl<S, M, E> IntoResponse for BinaryStream<S, M>
where
    S: futures_core::Stream<Item = Result<bytes::Bytes, E>> + Send + 'static,
    M: MediaType,
    E: Into<Box<dyn std::error::Error + Send + Sync>> + 'static,
{
    fn into_response(self) -> Response {
        let () = <M as WellFormed>::WELL_FORMED;

        with_content_type(
            Body::from_stream(self.stream),
            HeaderValue::from_static(M::MEDIA_TYPE),
        )
    }
}

impl<S, M> Responses for BinaryStream<S, M>
where
    S: futures_core::Stream,
    M: MediaType,
{
    // The empty Schema Object as `schema`, as for `Binary<M>`: raw bytes are
    // outside what JSON Schema describes, and `itemSchema` is defined only for
    // sequential media types, which an arbitrary `M` is not.
    fn responses(_registry: &mut Registry) -> kynos_openapi::Responses {
        let () = <M as WellFormed>::WELL_FORMED;
        ok(M::MEDIA_TYPE, unconstrained())
    }
}

#[cfg(test)]
mod tests;
