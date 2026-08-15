//! The `application/protobuf` body codec.

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

/// An `application/protobuf` request or response body.
///
/// Requires the `protobuf` feature. A missing or different content type
/// rejects with 415 and an invalid protobuf message rejects with 400.
///
/// ```no_run
/// # #[cfg(feature = "protobuf")]
/// # {
/// use kynos::extract::body::protobuf::Protobuf;
///
/// async fn echo<T>(Protobuf(message): Protobuf<T>) -> Protobuf<T> {
///     Protobuf(message)
/// }
/// # }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Protobuf<T>(pub T);

/// One spelling, read by both halves: what is decoded and what is described.
const MEDIA_TYPE: &str = "application/protobuf";

impl<C: Sync, T: prost::Message + Default + Send> FromRequest<C> for Protobuf<T> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, _context: &C) -> Result<Self, Self::Rejection> {
        let bytes = super::read_body(request, MEDIA_TYPE).await?;

        // Protobuf has no layer between the wire format and the message, so a
        // decode failure is always a malformed body rather than one that
        // parsed and then disagreed with the message definition.
        T::decode(bytes)
            .map(Self)
            .map_err(|error| BodyRejection::Syntax {
                detail: error.to_string(),
            })
    }
}

impl<T: Schema> Describe for Protobuf<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<T: Schema> RequestContent for Protobuf<T> {
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
