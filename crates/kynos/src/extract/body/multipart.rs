//! The `multipart/form-data` body codec.

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

/// A `multipart/form-data` request body with declared fields.
///
/// `T` derives `Schema`, and each field becomes a part with its own `Encoding`.
/// The same wrapper may be returned as a response, preserving the declared
/// field names, per-part media types, and encodings in both directions.
/// There is no dynamic-field iterator: a handler that accepts arbitrary part
/// names cannot describe them. For a variable number of uploads, declare one
/// field of type `Vec<FilePart>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultipartForm<T>(pub T);

/// One uploaded file within a [`MultipartForm`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePart {
    /// The client-supplied file name, if any.
    pub file_name: Option<String>,
    /// The declared media type of this part.
    pub content_type: Option<String>,
    /// The part's bytes.
    pub bytes: bytes::Bytes,
}

/// A part's bytes are raw binary, which sits outside JSON Schema's `type`
/// exactly as a raw binary message body does.
///
/// The part's media type is the Encoding Object's to state, and a
/// `contentMediaType` here would contradict it — which the specification says
/// is ignored. So the schema is the empty one, and every part-level fact is
/// carried where a consumer will actually read it. See `docs/schema.md`.
impl Schema for FilePart {
    fn schema(_registry: &mut Registry) -> kynos_openapi::Schema {
        kynos_openapi::Schema::Object(Box::default())
    }
}

impl<C: Sync, T: Send> FromRequest<C> for MultipartForm<T> {
    type Rejection = BodyRejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

impl<T: Schema> Describe for MultipartForm<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<T: Schema> RequestContent for MultipartForm<T> {
    fn media_types() -> Vec<&'static str> {
        vec!["multipart/form-data"]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        todo!()
    }
}
