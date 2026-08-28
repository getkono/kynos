//! Writing `application/json` as a response.
//!
//! [`Json`](crate::extract::body::json::Json) is declared once, on the
//! extracting side, because a codec is one type used in both directions. This
//! module adds the responding half and declares nothing, so it is private.

use crate::{
    error::problem::Problem,
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

use crate::extract::body::json::Json;

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
