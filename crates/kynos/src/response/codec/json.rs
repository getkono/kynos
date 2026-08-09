//! Writing `application/json` as a response.

use crate::{
    http::Response,
    response::{IntoResponse, Responses},
    schema::{Registry, Schema},
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
        todo!()
    }
}

impl<T: Schema> Responses for Json<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
