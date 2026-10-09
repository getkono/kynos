//! Writing `text/plain` as a response.

use kynos_openapi::model::body::mime_names;

use crate::{
    extract::body::text::Text,
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

impl IntoResponse for Text {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::from_bytes(bytes::Bytes::from(self.0)));
        // RFC 6657 removed `text/plain`'s US-ASCII default, so UTF-8 is stated.
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        response
    }
}

impl Responses for Text {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                mime_names::TEXT_PLAIN,
                kynos_openapi::MediaType::new(registry.resolve::<String>()),
            ),
        )
    }
}
