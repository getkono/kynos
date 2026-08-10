//! Raw bytes with a declared media type.

use crate::{
    error::rejection::Rejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
        media::MediaType,
    },
    http::Request,
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// A body of raw bytes with a declared media type.
///
/// `M` names the media type, so the description states what the bytes are
/// rather than shrugging. Binary content is described with
/// `contentMediaType`/`contentEncoding`, never the OpenAPI 3.0 `format: binary`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Binary<M>(pub bytes::Bytes, std::marker::PhantomData<M>);

impl<M> Binary<M> {
    /// Wraps bytes with their compile-time media type.
    pub fn new(bytes: impl Into<bytes::Bytes>) -> Self {
        Self(bytes.into(), std::marker::PhantomData)
    }
}

impl<C: Sync, M: MediaType + Send> FromRequest<C> for Binary<M> {
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

impl<M: MediaType> Describe for Binary<M> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<M: MediaType> RequestContent for Binary<M> {
    fn media_types() -> Vec<&'static str> {
        vec![M::MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        todo!()
    }
}
