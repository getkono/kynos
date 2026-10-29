//! A JSON body under a declared media type.

use std::marker::PhantomData;

use crate::{
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        describe::{Describe, RequestContent},
    },
    http::{
        Parts,
        body::Body,
        media::{MediaType, is_json},
    },
    router::operation::OperationCx,
    schema::{Schema, constraints::Pointer, registry::Registry},
};

/// A JSON request or response body carried under the media type `M` names.
///
/// Requires the default-on `json` feature. What [`Json`](super::json::Json)
/// is for `application/json`, this is for a vendor or versioned type such as
/// `application/vnd.acme.order.v2+json`: the same decoder, the same 400/422
/// split, and `T`'s schema in the description, keyed by `M::MEDIA_TYPE`
/// rather than `application/json`.
///
/// `M` names JSON: `application/json`, or an RFC 6839 `+json` type, compared
/// on type and subtype in any ASCII case. Any other marker fails the build
/// wherever the body is extracted, written, described or offered, rather than
/// compiling into a body no request reaches:
///
/// ```compile_fail
/// use kynos::{extract::{body::json_as::JsonAs, describe::RequestContent}, http::media::Png};
///
/// JsonAs::<Png, u64>::media_types();
/// ```
///
/// ```compile_fail
/// use kynos::{extract::body::json_as::JsonAs, http::media::Png, response::IntoResponse};
///
/// JsonAs::<Png, u64>::new(7).into_response();
/// ```
///
/// ```compile_fail
/// use kynos::{
///     extract::body::{json_as::JsonAs, text::Text},
///     http::media::Png,
///     response::negotiate::representation::Representations,
/// };
///
/// <(Text, JsonAs<Png, u64>)>::media_types();
/// ```
///
/// A request matches `M` by type and subtype, as for
/// [`Binary`](super::binary::Binary): it need not repeat a parameter the
/// marker declares, may not contradict one, and may add `charset=utf-8`. A
/// missing or different content type rejects with 415, so `application/json`
/// does not reach a `+json` body. Malformed JSON rejects with 400; valid JSON
/// that does not fit `T`, or breaks a bound its schema declares, with 422.
///
/// A response is serialized as `Json<T>`'s is, and carries `M::MEDIA_TYPE`
/// verbatim as its `Content-Type`.
///
/// ```no_run
/// use kynos::{extract::body::json_as::JsonAs, http::media::MediaType};
///
/// struct OrderV2;
/// impl MediaType for OrderV2 {
///     const MEDIA_TYPE: &'static str = "application/vnd.acme.order.v2+json";
/// }
///
/// async fn echo(order: JsonAs<OrderV2, String>) -> JsonAs<OrderV2, String> {
///     JsonAs::new(order.into_inner())
/// }
/// ```
///
/// Beside another body in a [`OneOf`](super::OneOf), the two must be offered
/// different requests, so two `JsonAs` of one type and subtype are refused,
///
/// ```compile_fail
/// use kynos::{
///     extract::{body::{OneOf, json_as::JsonAs}, describe::RequestContent},
///     http::media::MediaType,
/// };
///
/// struct V1;
/// impl MediaType for V1 {
///     const MEDIA_TYPE: &'static str = "application/vnd.acme+json; version=1";
/// }
/// struct V2;
/// impl MediaType for V2 {
///     const MEDIA_TYPE: &'static str = "application/VND.acme+json; version=2";
/// }
///
/// OneOf::<JsonAs<V1, u64>, JsonAs<V2, u64>>::media_types();
/// ```
///
/// as is one naming `application/json` beside a [`Json`](super::json::Json),
///
/// ```compile_fail
/// use kynos::{
///     extract::{body::{OneOf, json::Json, json_as::JsonAs}, describe::RequestContent},
///     http::media,
/// };
///
/// OneOf::<Json<u64>, JsonAs<media::Json, u64>>::media_types();
/// ```
///
/// and one beside a `Binary` of its own type and subtype:
///
/// ```compile_fail
/// use kynos::{
///     extract::{body::{OneOf, binary::Binary, json_as::JsonAs}, describe::RequestContent},
///     http::media::MediaType,
/// };
///
/// struct Order;
/// impl MediaType for Order {
///     const MEDIA_TYPE: &'static str = "application/vnd.acme.order+json";
/// }
///
/// OneOf::<JsonAs<Order, u64>, Binary<Order>>::media_types();
/// ```
///
/// The same holds for a response offer, which `Accept` tells apart by type and
/// subtype alone:
///
/// ```compile_fail
/// use kynos::{
///     extract::body::{json::Json, json_as::JsonAs},
///     http::media,
///     response::negotiate::representation::Representations,
/// };
///
/// <(Json<u64>, JsonAs<media::Json, u64>)>::media_types();
/// ```
///
/// Distinct types are alternatives, which is how one operation reads, and
/// offers, two versions of a document:
///
/// ```
/// use kynos::{
///     extract::{body::{OneOf, json::Json, json_as::JsonAs}, describe::RequestContent},
///     http::media::MediaType,
///     response::negotiate::representation::Representations,
/// };
///
/// struct V1;
/// impl MediaType for V1 {
///     const MEDIA_TYPE: &'static str = "application/vnd.acme.v1+json";
/// }
/// struct V2;
/// impl MediaType for V2 {
///     const MEDIA_TYPE: &'static str = "application/vnd.acme.v2+json";
/// }
///
/// let offered = OneOf::<OneOf<JsonAs<V1, u64>, JsonAs<V2, String>>, Json<u64>>::media_types();
/// assert_eq!(
///     offered,
///     ["application/vnd.acme.v1+json", "application/vnd.acme.v2+json", "application/json"]
/// );
///
/// let offered = <(Json<u64>, JsonAs<V1, u64>, JsonAs<V2, String>)>::media_types();
/// assert_eq!(offered.len(), 3);
///
/// let written = kynos::response::IntoResponse::into_response(JsonAs::<V1, u64>::new(7));
/// assert_eq!(written.headers()["content-type"], "application/vnd.acme.v1+json");
/// ```
pub struct JsonAs<M, T> {
    /// The decoded document.
    pub value: T,
    media: PhantomData<M>,
}

impl<M, T> JsonAs<M, T> {
    /// Wraps a document with its compile-time media type.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            value,
            media: PhantomData,
        }
    }

    /// Takes the document out.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

// By hand, so each is bounded on `T` alone: a media type is a marker, and a
// derive would ask it to be `Clone` or `Debug` too.
impl<M, T: Clone> Clone for JsonAs<M, T> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone())
    }
}

impl<M, T: std::fmt::Debug> std::fmt::Debug for JsonAs<M, T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JsonAs")
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}

impl<M, T: Default> Default for JsonAs<M, T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<M, T: PartialEq> PartialEq for JsonAs<M, T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<M, T: Eq> Eq for JsonAs<M, T> {}

impl<M: MediaType, T> JsonAs<M, T> {
    /// Refuses, in the constant evaluation reading it, a marker that does not
    /// name JSON.
    pub(crate) const JSON: () = assert!(
        is_json(M::MEDIA_TYPE),
        "a `JsonAs` marker names neither `application/json` nor a `+json` type, and its body \
         is decoded as JSON"
    );
}

/// `T: Schema` because the bounds a derived field declares are checked once `T`
/// is deserialized, as for `Json<T>`.
impl<C, M, T> FromRequest<C> for JsonAs<M, T>
where
    C: Sync,
    M: MediaType + Send,
    T: serde::de::DeserializeOwned + Schema + Send,
{
    type Rejection = BodyRejection;

    async fn from_request(
        parts: &mut Parts,
        body: Body,
        _context: &C,
    ) -> Result<Self, Self::Rejection> {
        let () = Self::JSON;

        let bytes = super::read_body(parts, body, M::MEDIA_TYPE).await?;
        let value =
            crate::extract::strict_json::from_slice(&bytes).map_err(super::json::rejection)?;
        super::checked(value, Pointer::root()).map(Self::new)
    }
}

impl<M: MediaType, T: Schema> Describe for JsonAs<M, T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<M: MediaType, T: Schema> RequestContent for JsonAs<M, T> {
    fn media_types() -> Vec<&'static str> {
        let () = Self::JSON;
        vec![M::MEDIA_TYPE]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let () = Self::JSON;
        kynos_openapi::RequestBody::new(
            M::MEDIA_TYPE,
            kynos_openapi::MediaType::new(registry.resolve::<T>()),
        )
    }
}
