//! The `application/protobuf` body codec.

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{Parts, body::Body},
    router::operation::OperationCx,
    schema::{Schema, constraints::Pointer, registry::Registry},
};

/// An `application/protobuf` request or response body.
///
/// Requires the `protobuf` feature. A missing or different content type
/// rejects with 415 and an invalid protobuf message rejects with 400, while a
/// message that decodes but breaks a bound `T`'s schema declares rejects with
/// 422, keyed by the JSON Pointer of each member that broke one under the
/// name the description gives it. Which bounds are enforced is
/// [`constraints`](crate::schema::constraints)' to say.
///
/// Extracting one therefore requires `T: Schema` as well as
/// `T: prost::Message`, as every codec that decodes a `Schema` type does.
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

/// The media type decoded, described and written by the response half.
pub(crate) const MEDIA_TYPE: &str = "application/protobuf";

/// `T: Schema` because the bounds a derived field declares are checked once `T`
/// is decoded.
impl<C: Sync, T: prost::Message + Default + Schema + Send> FromRequest<C> for Protobuf<T> {
    type Rejection = BodyRejection;

    async fn from_request(
        parts: &mut Parts,
        body: Body,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        let bytes = super::read_body(parts, body, MEDIA_TYPE).await?;

        // Bytes protobuf cannot decode are a 400; a decoded message that
        // breaks a bound is a 422.
        let value = T::decode(bytes).map_err(|error| BodyRejection::Syntax {
            detail: error.to_string(),
        })?;
        super::checked(value, Pointer::root()).map(Self)
    }
}

impl<T: Schema> Describe for Protobuf<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        super::describe_body::<Self>(operation);
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
