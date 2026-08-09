//! The `text/plain` body codec.

use crate::{
    error::Rejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::Request,
    router::OperationCx,
    schema::Registry,
};

/// A `text/plain` request or response body.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Text(pub String);

impl<C: Sync> FromRequest<C> for Text {
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

impl Describe for Text {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl RequestContent for Text {
    fn media_types() -> Vec<&'static str> {
        vec!["text/plain"]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        todo!()
    }
}
