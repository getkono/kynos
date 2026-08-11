//! Raw bytes with a declared media type.

use crate::{
    error::rejection::BodyRejection,
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
///
/// The media type is a marker rather than a field, so this is a named struct
/// and not the newtype every other extractor is: a handler binds the whole
/// value and reaches the bytes through [`into_inner`](Self::into_inner) or the
/// public field, rather than destructuring in the argument pattern.
///
/// ```no_run
/// use kynos::extract::{body::binary::Binary, media::Png};
///
/// async fn upload(body: Binary<Png>) -> kynos::response::status::NoContent {
///     let bytes = body.into_inner();
///     let _ = bytes;
///     kynos::response::status::NoContent
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Binary<M> {
    /// The body's bytes.
    pub bytes: bytes::Bytes,
    media: std::marker::PhantomData<M>,
}

impl<M> Binary<M> {
    /// Wraps bytes with their compile-time media type.
    pub fn new(bytes: impl Into<bytes::Bytes>) -> Self {
        Self {
            bytes: bytes.into(),
            media: std::marker::PhantomData,
        }
    }

    /// Takes the bytes out.
    #[must_use]
    pub fn into_inner(self) -> bytes::Bytes {
        self.bytes
    }
}

impl<C: Sync, M: MediaType + Send> FromRequest<C> for Binary<M> {
    type Rejection = BodyRejection;

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

    // Raw binary as a whole message body, which is the shape 3.1 describes by
    // *omitting* things. `type` is absent because raw binary is outside the
    // type system JSON Schema describes, and `contentMediaType` is absent
    // because it would only repeat the key this content sits under -- the
    // specification says a contradicting one is ignored, so the honest move is
    // not to write it twice. What is left is the empty Schema Object.
    //
    // Base64 in a *text* format is the other case, and it is not this one: that
    // is a `string` with `contentEncoding`, and it arises from a field inside a
    // JSON or form body rather than from the body itself.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        kynos_openapi::RequestBody::new(
            M::MEDIA_TYPE,
            kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
        )
    }
}
