//! Writing `text/plain` as a response.

use crate::{
    extract::body::text::Text,
    http::{HeaderValue, Response, body::Body, header},
    response::{IntoResponse, Responses},
    schema::registry::Registry,
};

impl IntoResponse for Text {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::from_bytes(bytes::Bytes::from(self.0)));
        // A Rust `String` is UTF-8, and RFC 6657 removed `text/plain`'s
        // US-ASCII default, so the charset is stated rather than assumed.
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
                "text/plain",
                kynos_openapi::MediaType::new(registry.resolve::<String>()),
            ),
        )
    }
}
