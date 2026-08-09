//! Writing `text/plain` as a response.

use crate::{
    extract::body::text::Text,
    http::Response,
    response::{IntoResponse, Responses},
    schema::Registry,
};

impl IntoResponse for Text {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl Responses for Text {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
