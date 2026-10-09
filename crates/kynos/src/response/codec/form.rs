//! Writing `application/x-www-form-urlencoded` as a response.

use kynos_openapi::model::body::mime_names;

use crate::{
    error::problem::Problem,
    extract::body::form::Form,
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

impl<T: serde::Serialize> IntoResponse for Form<T> {
    fn into_response(self) -> Response {
        // Encoded in full before anything is written, for the reason the JSON
        // codec gives: a failure that arrives mid-body has no status left.
        let Ok(encoded) = serde_html_form::to_string(&self.0) else {
            return Problem::new(StatusCode::INTERNAL_SERVER_ERROR)
                .with_detail("the response body could not be encoded as a form")
                .into_response();
        };

        let mut response = Response::new(Body::from_bytes(bytes::Bytes::from(encoded)));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(mime_names::APPLICATION_FORM_URLENCODED),
        );
        response
    }
}

impl<T: Schema> Responses for Form<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                mime_names::APPLICATION_FORM_URLENCODED,
                kynos_openapi::MediaType::new(registry.resolve::<T>()),
            ),
        )
    }
}
