//! Writing a value in the format an application's `BodyCodec` names as a
//! response.

use crate::{
    error::problem::Problem,
    extract::body::codec::{BodyCodec, Encoded},
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::{IntoResponse, Responses, codec::BodyWriter},
    schema::{Schema, registry::Registry},
};

impl<C: BodyCodec<T>, T> IntoResponse for Encoded<C, T> {
    fn into_response(self) -> Response {
        let () = Self::WELL_FORMED;

        // Encoded in full first, so a failure can still change the status.
        let mut writer = BodyWriter::new();
        if C::encode(&self.value, &mut writer).is_err() {
            return Problem::new(StatusCode::INTERNAL_SERVER_ERROR)
                .with_detail("the response body could not be serialized")
                .into_response();
        }

        let mut response = Response::new(Body::from_bytes(writer.into_bytes()));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(C::MEDIA_TYPE),
        );
        response
    }
}

impl<C: BodyCodec<T>, T: Schema> Responses for Encoded<C, T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let () = Self::WELL_FORMED;
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                C::MEDIA_TYPE,
                kynos_openapi::MediaType::new(registry.resolve::<T>()),
            ),
        )
    }
}
