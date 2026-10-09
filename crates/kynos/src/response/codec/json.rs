//! Writing `application/json` as a response.

use kynos_openapi::model::body::mime_names;

use crate::{
    error::problem::Problem,
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

use crate::extract::body::json::Json;

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        // Serialized in full first, so a failure can still change the status.
        let Ok(bytes) = serde_json::to_vec(&self.0) else {
            return Problem::new(StatusCode::INTERNAL_SERVER_ERROR)
                .with_detail("the response body could not be serialized")
                .into_response();
        };

        let mut response = Response::new(Body::from_bytes(bytes::Bytes::from(bytes)));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(mime_names::APPLICATION_JSON),
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
                mime_names::APPLICATION_JSON,
                kynos_openapi::MediaType::new(registry.resolve::<T>()),
            ),
        )
    }
}
