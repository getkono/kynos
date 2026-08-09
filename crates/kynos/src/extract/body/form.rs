//! The `application/x-www-form-urlencoded` body codec.

use crate::{
    error::Rejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::Request,
    router::operation::OperationCx,
    schema::{Registry, Schema},
};

/// An `application/x-www-form-urlencoded` request body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Form<T>(pub T);

impl<C: Sync, T: serde::de::DeserializeOwned + Send> FromRequest<C> for Form<T> {
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

impl<T: Schema> Describe for Form<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<T: Schema> RequestContent for Form<T> {
    fn media_types() -> Vec<&'static str> {
        vec!["application/x-www-form-urlencoded"]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        todo!()
    }
}
