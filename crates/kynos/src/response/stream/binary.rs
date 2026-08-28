//! Streamed binary content with a declared media type.

use crate::{
    extract::media::MediaType,
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

/// A streamed binary response with a declared media type.
///
/// Requires OpenAPI 3.2, whose sequential body vocabulary can describe a
/// representation processed incrementally. Stream failures terminate the body
/// because the successful status has already been committed.
///
/// ```no_run
/// # #[cfg(feature = "openapi32")]
/// # {
/// use kynos::{extract::media::OctetStream, response::stream::binary::BinaryStream};
///
/// fn download<S>(chunks: S) -> BinaryStream<S, OctetStream> {
///     BinaryStream::new(chunks)
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct BinaryStream<S, M> {
    /// The stream producing byte chunks.
    pub stream: S,
    media_type: std::marker::PhantomData<M>,
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
        // The chunks are already the bytes to send, so nothing here frames
        // them: the stream reaches the body unchanged.
        let mut response = Response::new(Body::from_stream(self.stream));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(M::MEDIA_TYPE),
        );
        response
    }
}

impl<S, M> Responses for BinaryStream<S, M>
where
    S: futures_core::Stream,
    M: MediaType,
{
    // `itemSchema` rather than `schema`, which is what 3.2 added the field for:
    // an item is described independently of the rest so a consumer can process
    // one as it arrives. Each item is the empty Schema Object for the reason a
    // non-streamed `Binary<M>` body is -- raw bytes sit outside the type system
    // JSON Schema describes, and calling them a `string` would be a lie.
    fn responses(_registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                M::MEDIA_TYPE,
                kynos_openapi::MediaType::sequential(kynos_openapi::Schema::Object(Box::default())),
            ),
        )
    }
}
