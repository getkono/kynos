//! Turning a request into a handler's arguments.
//!
//! # The rule that makes Kynos work
//!
//! There are two ways a handler argument can come into existence, and Kynos
//! keeps them apart:
//!
//! - It is derived from the **request**, in which case it implements
//!   [`FromRequestParts`] (or [`FromRequest`]) *and* [`Describe`], and
//!   contributes a Parameter or a Request Body to the description.
//! - It is derived from **application state**, in which case it implements
//!   [`FromContext`](crate::di::FromContext) and contributes nothing.
//!
//! Axum's single `FromRequestParts` conflates the two, which is exactly why
//! tools that infer a description from axum handlers produce documents with
//! silent holes. Keeping them apart is what lets Kynos guarantee there are
//! none.
//!
//! A consequence worth stating plainly: **there is no extractor that yields the
//! whole request**. No `Request`, no `Body`, no `HeaderMap`. Those are the
//! holes. A handler that wants an arbitrary header declares it with
//! [`Headers`]; a handler that wants an arbitrary body says
//! [`Unchecked`](crate::schema::Unchecked).
//!
//! # Rejections describe themselves too
//!
//! [`FromRequestParts::Rejection`] is bound by
//! [`Responses`](crate::response::Responses), so every way an extractor can
//! fail appears in the operation's `responses`.

use std::future::Future;

use crate::{
    error::Rejection,
    http::{Parts, Request},
    router::OperationCx,
    schema::{Registry, Schema},
};

/// A handler input built from the request head.
///
/// Every implementation must also implement [`Describe`]; the two are separate
/// traits only because the runtime half is generic over the application context
/// and the describing half is not.
pub trait FromRequestParts<C>: Sized + Send {
    /// How this extractor fails, and what that failure looks like in the
    /// description.
    type Rejection: crate::response::IntoResponse + crate::response::Responses;

    /// Extracts the value, or explains why it could not.
    fn from_request_parts(
        parts: &mut Parts,
        context: &C,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send;
}

/// A handler input that consumes the request body.
///
/// At most one argument per handler may implement this, and it must be the
/// last. The split from [`FromRequestParts`] is what enforces that: the body
/// can only be taken once, so the type system takes it once.
pub trait FromRequest<C>: Sized + Send {
    /// How this extractor fails, and what that failure looks like in the
    /// description.
    type Rejection: crate::response::IntoResponse + crate::response::Responses;

    /// Consumes the request, or explains why it could not.
    fn from_request(
        request: Request,
        context: &C,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send;
}

/// What a request-derived input contributes to the description.
///
/// This is the trait that makes an undescribable handler fail to compile: there
/// is no blanket implementation, and no way to write one for a type that cannot
/// say what it reads.
pub trait Describe {
    /// Adds this input's parameters or request body to the operation.
    fn describe(operation: &mut OperationCx<'_>);
}

/// Metadata shared by request-body extractors.
///
/// This trait deliberately has no implementation for request-part extractors.
/// Consequently `Option<Path<T>>` and similar ambiguous signatures do not
/// compile, while `Option<Json<T>>` means that the entire body is optional.
///
/// ```compile_fail
/// fn body<T: kynos::extract::FromRequest<()>>() {}
/// body::<Option<kynos::extract::Path<u64>>>();
/// ```
pub trait RequestContent: Describe {
    /// Every media type accepted by this body extractor.
    fn media_types() -> Vec<&'static str>;

    /// Builds the required OpenAPI Request Body Object for this extractor.
    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody;
}

/// One of two request body representations, selected by `Content-Type`.
///
/// The alternatives must implement [`Alternative`], which is provided only
/// for pairs whose media types are known to be distinct. Unsupported media
/// types reject with 415; a malformed selected representation uses that
/// representation's normal rejection.
///
/// ```no_run
/// use kynos::extract::{Binary, OneOf, Text, media::Pdf};
///
/// async fn upload(body: OneOf<Text, Binary<Pdf>>) {
///     match body {
///         OneOf::Left(text) => drop(text),
///         OneOf::Right(pdf) => drop(pdf),
///     }
/// }
/// ```
///
/// Alternatives with the same media type are intentionally not implemented:
///
/// ```compile_fail
/// fn body<T: kynos::extract::FromRequest<()>>() {}
/// body::<kynos::extract::OneOf<kynos::extract::Text, kynos::extract::Text>>();
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OneOf<L, R> {
    /// The left representation was selected.
    Left(L),
    /// The right representation was selected.
    Right(R),
}

/// Proves that two request content types can be alternatives.
///
/// Kynos implements this for its non-overlapping body wrappers. It is not a
/// blanket trait: writing `OneOf<Json<A>, Json<B>>` therefore fails to compile
/// instead of making dispatch order observable.
pub trait Alternative<Rhs>: RequestContent
where
    Rhs: RequestContent,
{
}

/// Variables captured from the path template.
///
/// `T` derives `PathParams`, and its field names are checked against the route
/// template at compile time — a mismatch is a compile error, not a runtime 500,
/// which is the failure mode every other Rust framework has here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path<T>(pub T);

/// Named query string parameters.
///
/// `T` derives `QueryParams`. Nested objects are rejected at compile time:
/// `deepObject` is defined only for objects whose properties are scalars, so a
/// deeper shape has no legal serialization. Under `openapi32`, reach for
/// [`QueryString`] instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Query<T>(pub T);

/// The whole query string, described by media type.
///
/// Introduced by OpenAPI 3.2's `in: querystring`. This is the sanctioned way to
/// describe search filters, JSON in the query, or RFC 9535 JSONPath — shapes a
/// list of named parameters cannot express. It must be the only query-related
/// input on its handler.
#[cfg(feature = "openapi32")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryString<T, M>(pub T, std::marker::PhantomData<M>);

#[cfg(feature = "openapi32")]
impl<T, M> QueryString<T, M> {
    /// Wraps a decoded whole-query-string value with its declared media type.
    pub fn new(value: T) -> Self {
        Self(value, std::marker::PhantomData)
    }
}

/// Declared request headers.
///
/// `T` derives `Headers`. Declaring `Accept`, `Content-Type` or `Authorization`
/// is a compile error: the specification says a parameter definition for those
/// is ignored, so accepting one would put a claim in the description that no
/// consumer will honour. Use content negotiation for the first two and
/// [`Auth`](crate::security::Auth) for the third.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Headers<T>(pub T);

/// Declared request cookies.
///
/// `T` derives `Cookies`. There is no whole-jar extractor; a cookie carrying
/// credentials is a [`SecurityScheme`](crate::security::SecurityScheme), not a
/// parameter.
#[cfg(feature = "cookie")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cookies<T>(pub T);

/// An `application/json` request or response body.
///
/// Requires the default-on `json` feature. Requests accept
/// `application/json` with no parameters or with `charset=utf-8`; a missing or
/// different content type rejects with 415. Malformed or incomplete JSON
/// rejects with 400, while valid JSON that cannot deserialize into `T` or
/// violates derived schema constraints rejects with 422.
///
/// ```no_run
/// use kynos::extract::Json;
///
/// async fn echo(Json(message): Json<String>) -> Json<String> {
///     Json(message)
/// }
/// ```
#[cfg(feature = "json")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Json<T>(pub T);

/// An `application/protobuf` request or response body.
///
/// Requires the `protobuf` feature. A missing or different content type
/// rejects with 415 and an invalid protobuf message rejects with 400.
///
/// ```no_run
/// # #[cfg(feature = "protobuf")]
/// # {
/// use kynos::extract::Protobuf;
///
/// async fn echo<T>(Protobuf(message): Protobuf<T>) -> Protobuf<T> {
///     Protobuf(message)
/// }
/// # }
/// ```
#[cfg(feature = "protobuf")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Protobuf<T>(pub T);

/// An `application/x-www-form-urlencoded` request body.
#[cfg(feature = "form")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Form<T>(pub T);

/// A `multipart/form-data` request body with declared fields.
///
/// `T` derives `Schema`, and each field becomes a part with its own `Encoding`.
/// The same wrapper may be returned as a response, preserving the declared
/// field names, per-part media types, and encodings in both directions.
/// There is no dynamic-field iterator: a handler that accepts arbitrary part
/// names cannot describe them. For a variable number of uploads, declare one
/// field of type `Vec<FilePart>`.
#[cfg(feature = "multipart")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultipartForm<T>(pub T);

/// One uploaded file within a [`MultipartForm`].
#[cfg(feature = "multipart")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePart {
    /// The client-supplied file name, if any.
    pub file_name: Option<String>,
    /// The declared media type of this part.
    pub content_type: Option<String>,
    /// The part's bytes.
    pub bytes: bytes::Bytes,
}

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

/// A `text/plain` request or response body.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Text(pub String);

/// The path template this request matched.
///
/// Exactly the `paths` key from the description, which makes it the correct
/// label for a metric — unlike the concrete URI, it has bounded cardinality.
/// Contributes nothing to the description.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatchedPath(pub &'static str);

/// The peer address of the connection this request arrived on.
///
/// Contributes nothing to the description: it is a property of the connection,
/// not of the API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectInfo(pub std::net::SocketAddr);

/// A media type usable as the `M` parameter of [`Binary`] or [`QueryString`].
///
/// Implemented by the marker types in [`media`], and by any unit struct you
/// declare for a vendor type.
pub trait MediaType {
    /// The media type, as it appears in a `Content-Type` header.
    const MEDIA_TYPE: &'static str;
}

/// Marker types naming common media types.
pub mod media {
    use super::MediaType;

    /// `application/octet-stream`.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct OctetStream;

    impl MediaType for OctetStream {
        const MEDIA_TYPE: &'static str = "application/octet-stream";
    }

    /// `application/pdf`.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Pdf;

    impl MediaType for Pdf {
        const MEDIA_TYPE: &'static str = "application/pdf";
    }

    /// `image/png`.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Png;

    impl MediaType for Png {
        const MEDIA_TYPE: &'static str = "image/png";
    }

    /// `application/json`, for a query string described as JSON.
    #[cfg(feature = "json")]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Json;

    #[cfg(feature = "json")]
    impl MediaType for Json {
        const MEDIA_TYPE: &'static str = "application/json";
    }
}

/// A group of path parameters.
///
/// Derived, never implemented by hand. [`NAMES`](PathParams::NAMES) is what the
/// route attribute compares against the path template.
pub trait PathParams: Sized {
    /// The parameter names, in declaration order.
    const NAMES: &'static [&'static str];

    /// Decodes the named captures from a matched route.
    fn decode(values: &[(&str, &str)]) -> Result<Self, Rejection> {
        let _ = values;
        todo!()
    }

    /// Encodes this value for a typed endpoint URI.
    fn encode(&self) -> Vec<(&'static str, String)> {
        todo!()
    }

    /// Describes each captured value as an OpenAPI path parameter.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        todo!()
    }
}

/// A group of query parameters.
pub trait QueryParams: Sized + Schema {
    /// Decodes a raw query string.
    fn decode(query: Option<&str>) -> Result<Self, Rejection> {
        let _ = query;
        todo!()
    }

    /// Encodes this value as a query string without the leading `?`.
    fn encode(&self) -> String {
        todo!()
    }

    /// Describes the individual OpenAPI query parameters.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        todo!()
    }
}

/// A group of declared request or response headers.
///
/// The same derived contract is used by [`Headers`] while extracting and by
/// [`WithHeaders`](crate::response::WithHeaders) while responding. Encoding
/// returns a sequence rather than a map so fields such as `Set-Cookie` can be
/// emitted more than once without comma joining.
pub trait HeaderParams: Sized {
    /// The header names this group declares.
    const NAMES: &'static [&'static str];

    /// Decodes this group from request headers.
    fn decode(headers: &crate::http::HeaderMap) -> Result<Self, Rejection> {
        let _ = headers;
        todo!()
    }

    /// Encodes this group as response header values.
    fn encode(&self) -> Vec<(crate::http::HeaderName, crate::http::HeaderValue)> {
        todo!()
    }

    /// Describes the declared OpenAPI header parameters.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        todo!()
    }

    /// Describes the headers when this group is attached to a response.
    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;
        todo!()
    }
}

/// A group of request cookies.
pub trait CookieParams: Sized {
    /// The cookie names this group declares.
    const NAMES: &'static [&'static str];

    /// Decodes this group from the request's cookie header fields.
    fn decode(headers: &crate::http::HeaderMap) -> Result<Self, Rejection> {
        let _ = headers;
        todo!()
    }

    /// Describes the declared OpenAPI cookie parameters.
    fn parameters(registry: &mut Registry) -> Vec<kynos_openapi::Parameter> {
        let _ = registry;
        todo!()
    }
}

impl<C: Sync, T: PathParams + Send> FromRequestParts<C> for Path<T> {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl<T: PathParams + Schema> Describe for Path<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}

impl<C: Sync, T: QueryParams + Send> FromRequestParts<C> for Query<T> {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl<T: QueryParams> Describe for Query<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}

#[cfg(feature = "openapi32")]
impl<C: Sync, T: Send, M: MediaType + Send> FromRequestParts<C> for QueryString<T, M> {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

#[cfg(feature = "openapi32")]
impl<T: Schema, M: MediaType> Describe for QueryString<T, M> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}

impl<C: Sync, T: HeaderParams + Send> FromRequestParts<C> for Headers<T> {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl<T: HeaderParams> Describe for Headers<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}

#[cfg(feature = "cookie")]
impl<C: Sync, T: CookieParams + Send> FromRequestParts<C> for Cookies<T> {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

#[cfg(feature = "cookie")]
impl<T: CookieParams> Describe for Cookies<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
        todo!()
    }
}

#[cfg(feature = "json")]
impl<C: Sync, T: serde::de::DeserializeOwned + Send> FromRequest<C> for Json<T> {
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

#[cfg(feature = "json")]
impl<T: Schema> Describe for Json<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

#[cfg(feature = "json")]
impl<T: Schema> RequestContent for Json<T> {
    fn media_types() -> Vec<&'static str> {
        vec!["application/json"]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        kynos_openapi::RequestBody::json(registry.resolve::<T>())
    }
}

#[cfg(feature = "protobuf")]
impl<C: Sync, T: prost::Message + Default + Send> FromRequest<C> for Protobuf<T> {
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

#[cfg(feature = "protobuf")]
impl<T: Schema> Describe for Protobuf<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

#[cfg(feature = "protobuf")]
impl<T: Schema> RequestContent for Protobuf<T> {
    fn media_types() -> Vec<&'static str> {
        vec!["application/protobuf"]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        todo!()
    }
}

#[cfg(feature = "form")]
impl<C: Sync, T: serde::de::DeserializeOwned + Send> FromRequest<C> for Form<T> {
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

#[cfg(feature = "form")]
impl<T: Schema> Describe for Form<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

#[cfg(feature = "form")]
impl<T: Schema> RequestContent for Form<T> {
    fn media_types() -> Vec<&'static str> {
        vec!["application/x-www-form-urlencoded"]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        todo!()
    }
}

#[cfg(feature = "multipart")]
impl<C: Sync, T: Send> FromRequest<C> for MultipartForm<T> {
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

#[cfg(feature = "multipart")]
impl<T: Schema> Describe for MultipartForm<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

#[cfg(feature = "multipart")]
impl<T: Schema> RequestContent for MultipartForm<T> {
    fn media_types() -> Vec<&'static str> {
        vec!["multipart/form-data"]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        todo!()
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

impl<C: Sync> FromRequest<C> for Text {
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

impl Describe for Text {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl RequestContent for Text {
    fn media_types() -> Vec<&'static str> {
        vec!["text/plain"]
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let _ = registry;
        todo!()
    }
}

impl<C, T> FromRequest<C> for Option<T>
where
    C: Sync,
    T: FromRequest<C> + RequestContent,
{
    type Rejection = T::Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

impl<T: RequestContent> Describe for Option<T> {
    fn describe(operation: &mut OperationCx<'_>) {
        let body = T::request_body(operation.registry()).optional();
        operation.set_request_body(body);
    }
}

impl<C, L, R> FromRequest<C> for OneOf<L, R>
where
    C: Sync,
    L: FromRequest<C, Rejection = Rejection> + Alternative<R>,
    R: FromRequest<C, Rejection = Rejection> + RequestContent,
{
    type Rejection = Rejection;

    async fn from_request(request: Request, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (request, context);
        todo!()
    }
}

impl<L, R> Describe for OneOf<L, R>
where
    L: Alternative<R>,
    R: RequestContent,
{
    fn describe(operation: &mut OperationCx<'_>) {
        let body = <Self as RequestContent>::request_body(operation.registry());
        operation.set_request_body(body);
    }
}

impl<L, R> RequestContent for OneOf<L, R>
where
    L: Alternative<R>,
    R: RequestContent,
{
    fn media_types() -> Vec<&'static str> {
        let mut media_types = L::media_types();
        media_types.extend(R::media_types());
        media_types
    }

    fn request_body(registry: &mut Registry) -> kynos_openapi::RequestBody {
        let mut body = L::request_body(registry);
        for (media_type, content) in R::request_body(registry).content {
            assert!(
                body.content.insert(media_type.clone(), content).is_none(),
                "request body alternative repeats media type `{media_type}`"
            );
        }
        body
    }
}

impl<L, R, N> Alternative<N> for OneOf<L, R>
where
    L: Alternative<R>,
    R: RequestContent,
    N: RequestContent,
{
}

#[cfg(feature = "json")]
impl<T: Schema> Alternative<Text> for Json<T> {}
#[cfg(feature = "json")]
impl<T: Schema> Alternative<Json<T>> for Text {}
#[cfg(feature = "json")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Json<T> {}
#[cfg(feature = "json")]
impl<T: Schema, M: MediaType> Alternative<Json<T>> for Binary<M> {}

#[cfg(feature = "form")]
impl<T: Schema> Alternative<Text> for Form<T> {}
#[cfg(feature = "form")]
impl<T: Schema> Alternative<Form<T>> for Text {}
#[cfg(feature = "form")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Form<T> {}
#[cfg(feature = "form")]
impl<T: Schema, M: MediaType> Alternative<Form<T>> for Binary<M> {}

#[cfg(all(feature = "json", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<Form<U>> for Json<T> {}
#[cfg(all(feature = "json", feature = "form"))]
impl<T: Schema, U: Schema> Alternative<Json<U>> for Form<T> {}

#[cfg(feature = "multipart")]
impl<T: Schema> Alternative<Text> for MultipartForm<T> {}
#[cfg(feature = "multipart")]
impl<T: Schema> Alternative<MultipartForm<T>> for Text {}
#[cfg(feature = "multipart")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for MultipartForm<T> {}
#[cfg(feature = "multipart")]
impl<T: Schema, M: MediaType> Alternative<MultipartForm<T>> for Binary<M> {}

#[cfg(all(feature = "json", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<MultipartForm<U>> for Json<T> {}
#[cfg(all(feature = "json", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<Json<U>> for MultipartForm<T> {}

#[cfg(all(feature = "form", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<MultipartForm<U>> for Form<T> {}
#[cfg(all(feature = "form", feature = "multipart"))]
impl<T: Schema, U: Schema> Alternative<Form<U>> for MultipartForm<T> {}

#[cfg(feature = "protobuf")]
impl<T: Schema> Alternative<Text> for Protobuf<T> {}
#[cfg(feature = "protobuf")]
impl<T: Schema> Alternative<Protobuf<T>> for Text {}
#[cfg(feature = "protobuf")]
impl<T: Schema, M: MediaType> Alternative<Binary<M>> for Protobuf<T> {}
#[cfg(feature = "protobuf")]
impl<T: Schema, M: MediaType> Alternative<Protobuf<T>> for Binary<M> {}

#[cfg(all(feature = "json", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Protobuf<U>> for Json<T> {}
#[cfg(all(feature = "json", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Json<U>> for Protobuf<T> {}

#[cfg(all(feature = "form", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Protobuf<U>> for Form<T> {}
#[cfg(all(feature = "form", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Form<U>> for Protobuf<T> {}

#[cfg(all(feature = "multipart", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<Protobuf<U>> for MultipartForm<T> {}
#[cfg(all(feature = "multipart", feature = "protobuf"))]
impl<T: Schema, U: Schema> Alternative<MultipartForm<U>> for Protobuf<T> {}

impl<C: Sync> FromRequestParts<C> for MatchedPath {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl Describe for MatchedPath {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}

impl<C: Sync> FromRequestParts<C> for ConnectInfo {
    type Rejection = Rejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!()
    }
}

impl Describe for ConnectInfo {
    fn describe(operation: &mut OperationCx<'_>) {
        let _ = operation;
    }
}
