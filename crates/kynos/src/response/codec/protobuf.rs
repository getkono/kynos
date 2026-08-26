//! Writing `application/protobuf` as a response.

use crate::{
    extract::body::protobuf::Protobuf,
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

impl<T: prost::Message> IntoResponse for Protobuf<T> {
    fn into_response(self) -> Response {
        // Encoding a message into a growable buffer cannot fail: the only error
        // `prost` reports is insufficient capacity, which is what growing is.
        let encoded = self.0.encode_to_vec();

        let mut response = Response::new(Body::from_bytes(bytes::Bytes::from(encoded)));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/protobuf"),
        );
        response
    }
}

impl<T: Schema> Responses for Protobuf<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                "application/protobuf",
                kynos_openapi::MediaType::new(registry.resolve::<T>()),
            ),
        )
    }
}
