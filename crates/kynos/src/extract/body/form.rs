//! The `application/x-www-form-urlencoded` body codec.

use std::collections::BTreeMap;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::Request,
    router::operation::OperationCx,
    schema::{Schema, registry::Registry},
};

/// An `application/x-www-form-urlencoded` request body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Form<T>(pub T);

/// One spelling, read by both halves: what is decoded and what is described.
const MEDIA_TYPE: &str = "application/x-www-form-urlencoded";

impl<C: Sync, T: serde::de::DeserializeOwned + Send> FromRequest<C> for Form<T> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, _context: &C) -> Result<Self, Self::Rejection> {
        let bytes = super::read_body(request, MEDIA_TYPE).await?;

        // Form syntax admits no malformed input -- an unpaired key is a key
        // with an empty value, and a bad escape decodes lossily -- so every way
        // this fails is a pair that does not fit `T`, which is a 422 rather
        // than a 400. The failure is keyed by the root JSON Pointer because
        // serde reports which field only inside its message.
        serde_urlencoded::from_bytes(&bytes)
            .map(Self)
            .map_err(|error| BodyRejection::Schema {
                failures: BTreeMap::from([(String::new(), error.to_string())]),
            })
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
        vec![MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<T>()),
        )
    }
}
