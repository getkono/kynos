//! Writing raw bytes with a declared media type as a response.

use crate::{
    extract::{body::binary::Binary, media::MediaType},
    http::Response,
    response::{IntoResponse, Responses},
    schema::Registry,
};

impl<M: MediaType> IntoResponse for Binary<M> {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<M: MediaType> Responses for Binary<M> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
