//! Writing `application/protobuf` as a response.

use crate::{
    extract::body::protobuf::{MEDIA_TYPE, Message, Protobuf},
    http::{HeaderValue, Response, body::Body},
    response::{
        IntoResponse, Responses,
        codec::{ok, with_content_type},
        negotiate::representation::{Representation, sealed::SealedRepresentation},
    },
    schema::{Schema, registry::Registry},
};

impl<T: Message> IntoResponse for Protobuf<T> {
    fn into_response(self) -> Response {
        let encoded = self.0.encode();

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

// Bound through the codec, so the bound is stated once.
impl<T> Representation for Protobuf<T> where Protobuf<T>: IntoResponse + Responses {}
