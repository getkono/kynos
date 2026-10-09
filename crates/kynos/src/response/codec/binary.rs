//! Writing raw bytes with a declared media type as a response.

use crate::{
    extract::body::binary::Binary,
    http::{HeaderValue, Response, body::Body, header, media::MediaType},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

impl<M: MediaType> IntoResponse for Binary<M> {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::from_bytes(self.bytes));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(M::MEDIA_TYPE),
        );
        response
    }
}

impl<M: MediaType> Responses for Binary<M> {
    // The empty Schema Object: raw bytes are outside what JSON Schema describes,
    // and `contentMediaType` would only repeat the content key.
    fn responses(_registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                M::MEDIA_TYPE,
                kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
            ),
        )
    }
}
