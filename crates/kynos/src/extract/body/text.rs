//! The `text/plain` body codec.

use bytes::Bytes;
use kynos_openapi::model::body::mime_names;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{Parts, body::Body},
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// A `text/plain` request or response body.
///
/// Another `text/*` type is [`TextAs`](super::text_as::TextAs), the same
/// codec under the media type its marker names.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Text(pub String);

/// The media type decoded and described.
pub(super) const MEDIA_TYPE: &str = mime_names::TEXT_PLAIN;

impl<C: Sync> FromRequest<C> for Text {
    type Rejection = BodyRejection;

    async fn from_request(
        parts: &mut Parts,
        body: Body,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        let bytes = super::read_body(parts, body, MEDIA_TYPE).await?;
        decode(bytes).map(Self)
    }
}

/// A text body's octets as the string they encode: only UTF-8 is accepted, so
/// other bytes are a 400.
pub(super) fn decode(bytes: Bytes) -> Result<String, BodyRejection> {
    String::from_utf8(bytes.into()).map_err(|error| BodyRejection::Syntax {
        detail: error.to_string(),
    })
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

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::new(
            MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<String>()),
        )
    }
}
