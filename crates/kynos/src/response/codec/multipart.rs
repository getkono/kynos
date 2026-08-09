//! Writing `multipart/form-data` as a response.
//!
//! The declared field names, per-part media types and encodings are preserved
//! in both directions, so a `MultipartForm<T>` returned from a handler
//! describes the same parts it would accept.

use crate::{
    extract::body::multipart::MultipartForm,
    http::Response,
    response::{IntoResponse, Responses},
    schema::{Registry, Schema},
};

impl<T: Schema> IntoResponse for MultipartForm<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<T: Schema> Responses for MultipartForm<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
