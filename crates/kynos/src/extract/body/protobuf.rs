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

impl<C: Sync, T: prost::Message + Default + Send> FromRequest<C> for Protobuf<T> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
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
        vec!["application/protobuf"]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        todo!()
    }
}
