//! Writing `application/x-www-form-urlencoded` as a response.

use crate::{
    extract::body::form::Form,
    http::Response,
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

impl<T: serde::Serialize> IntoResponse for Form<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<T: Schema> Responses for Form<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
