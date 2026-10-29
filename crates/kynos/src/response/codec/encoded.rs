//! Writing a value in the format an application's `BodyCodec` names as a
//! response.

use crate::{
    extract::body::codec::{BodyCodec, Encoded},
    http::{HeaderValue, Response, body::Body},
    response::{
        IntoResponse, Responses,
        codec::{BodyWriter, ok, unserializable, with_content_type},
    },
    schema::{Schema, registry::Registry},
};

impl<C: BodyCodec<T>, T> IntoResponse for Encoded<C, T> {
    fn into_response(self) -> Response {
        let () = Self::WELL_FORMED;

        // Encoded in full first, so a failure can still change the status.
        let mut writer = BodyWriter::new();
        if C::encode(&self.value, &mut writer).is_err() {
            return unserializable("the response body could not be serialized");
        }

        with_content_type(
            Body::from_bytes(writer.into_bytes()),
            HeaderValue::from_static(C::MEDIA_TYPE),
        )
    }
}

impl<C: BodyCodec<T>, T: Schema> Responses for Encoded<C, T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let () = Self::WELL_FORMED;
        ok(
            C::MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<T>()),
        )
    }
}
