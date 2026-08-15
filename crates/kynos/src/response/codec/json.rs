//! Writing `application/json` as a response.

use crate::{
    error::problem::Problem,
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

/// A JSON response body, with status 200.
///
/// Requires the default-on `json` feature. Serialization is completed before
/// the response is committed, so a serialization failure becomes a documented
/// RFC 9457 500 response rather than a truncated successful response.
///
/// This is the same type a handler extracts with; the alias exists so that a
/// handler's return type reads as a response.
pub use crate::extract::body::json::Json;

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        // Serialized in full before anything is written, which is what leaves a
        // failure a status to spend: a body written as it serializes has already
        // committed the 200 it would then have to retract.
        let Ok(bytes) = serde_json::to_vec(&self.0) else {
            return Problem::new(StatusCode::INTERNAL_SERVER_ERROR)
                .with_detail("the response body could not be serialized")
                .into_response();
        };

        let mut response = Response::new(Body::from_bytes(bytes::Bytes::from(bytes)));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        response
    }
}

impl<T: Schema> Responses for Json<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                "application/json",
                kynos_openapi::MediaType::new(registry.resolve::<T>()),
            ),
        )
    }
}
