//! The `text/plain` body codec.

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::Request,
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// A `text/plain` request or response body.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Text(pub String);

/// One spelling, read by both halves: what is decoded and what is described.
const MEDIA_TYPE: &str = "text/plain";

impl<C: Sync> FromRequest<C> for Text {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, _context: &C) -> Result<Self, Self::Rejection> {
        let bytes = super::read_body(request, MEDIA_TYPE).await?;

        // The body was accepted as UTF-8 or as unparameterized `text/plain`, so
        // bytes that are not UTF-8 are a body that does not say what it claims.
        String::from_utf8(bytes.into())
            .map(Self)
            .map_err(|error| BodyRejection::Syntax {
                detail: error.to_string(),
            })
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
        vec![MEDIA_TYPE]
    }

    // The body is a string, so it is described by the schema `String` already
    // carries rather than by a second, hand-written one.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<String>()),
        )
    }
}
