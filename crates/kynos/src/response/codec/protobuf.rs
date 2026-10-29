//! Writing `application/protobuf` as a response.

use crate::{
    extract::body::protobuf::{MEDIA_TYPE, Protobuf},
    http::{HeaderValue, Response, body::Body},
    response::{
        IntoResponse, Responses,
        codec::{ok, with_content_type},
        negotiate::representation::{Representation, sealed::SealedRepresentation},
    },
    schema::{Schema, registry::Registry},
};

impl<T: prost::Message> IntoResponse for Protobuf<T> {
    fn into_response(self) -> Response {
        // Infallible: `prost`'s only error is insufficient capacity.
        let encoded = self.0.encode_to_vec();

        with_content_type(
            Body::from_bytes(bytes::Bytes::from(encoded)),
            HeaderValue::from_static(MEDIA_TYPE),
        )
    }
}

impl<T: Schema> Responses for Protobuf<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        ok(
            MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<T>()),
        )
    }
}

impl<T> SealedRepresentation for Protobuf<T> {
    const MEDIA_TYPE: &'static str = MEDIA_TYPE;
}

// Bound through the codec so the protobuf crate stays named only where
// `containment:check` allows it.
impl<T> Representation for Protobuf<T> where Protobuf<T>: IntoResponse + Responses {}
