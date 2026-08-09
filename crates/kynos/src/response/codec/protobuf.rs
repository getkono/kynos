//! Writing `application/protobuf` as a response.

use crate::{
    extract::body::protobuf::Protobuf,
    http::Response,
    response::{IntoResponse, Responses},
    schema::{Registry, Schema},
};

impl<T: prost::Message> IntoResponse for Protobuf<T> {
    fn into_response(self) -> Response {
        todo!()
    }
}

impl<T: Schema> Responses for Protobuf<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        let _ = registry;
        todo!()
    }
}
