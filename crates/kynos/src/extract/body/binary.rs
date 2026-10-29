//! Raw bytes with a declared media type.

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{
        Parts,
        body::Body,
        media::{MediaType, WellFormed},
    },
    router::operation::OperationCx,
    schema::registry::Registry,
};

/// A body of raw bytes with a declared media type.
///
/// `M` names the media type, so the description states what the bytes are
/// rather than shrugging. The schema is the empty one, since the media type key
/// already states what the bytes are; never the OpenAPI 3.0 `format: binary`.
///
/// A request matches `M` by type and subtype. It need not repeat a parameter
/// the marker declares, but may not contradict one: a marker declaring
/// `charset=iso-8859-1` refuses `charset=utf-8`.
///
/// Reach the bytes through [`into_inner`](Self::into_inner) or the public field.
///
/// ```no_run
/// use kynos::{extract::body::binary::Binary, http::media::Png};
///
/// async fn upload(body: Binary<Png>) -> kynos::response::status::NoContent {
///     let bytes = body.into_inner();
///     let _ = bytes;
///     kynos::response::status::NoContent
/// }
/// ```
pub struct Binary<M> {
    /// The body's bytes.
    pub bytes: bytes::Bytes,
    media: std::marker::PhantomData<M>,
}

impl<M> Binary<M> {
    /// Wraps bytes with their compile-time media type.
    #[must_use]
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

// By hand, so none is bounded on `M`: a media type is a marker, and a derive
// would ask it to be `Clone` or `Debug` too.
impl<M> Clone for Binary<M> {
    fn clone(&self) -> Self {
        Self::new(self.bytes.clone())
    }
}

impl<M> std::fmt::Debug for Binary<M> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Binary")
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl<M> Default for Binary<M> {
    fn default() -> Self {
        Self::new(bytes::Bytes::new())
    }
}

impl<M> PartialEq for Binary<M> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl<M> Eq for Binary<M> {}

impl<C: Sync, M: MediaType + Send> FromRequest<C> for Binary<M> {
    type Rejection = BodyRejection;

    async fn from_request(
        parts: &mut Parts,
        body: Body,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        let () = <M as WellFormed>::WELL_FORMED;

        super::read_body(parts, body, M::MEDIA_TYPE)
            .await
            .map(Self::new)
    }
}

impl<M: MediaType> Describe for Binary<M> {
    fn describe(operation: &mut OperationCx<'_>) {
        super::describe_body::<Self>(operation);
    }
}

impl<M: MediaType> RequestContent for Binary<M> {
    fn media_types() -> Vec<&'static str> {
        let () = <M as WellFormed>::WELL_FORMED;
        vec![M::MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        let () = <M as WellFormed>::WELL_FORMED;
        kynos_openapi::RequestBody::new(
            M::MEDIA_TYPE,
            kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())),
        )
    }
}
