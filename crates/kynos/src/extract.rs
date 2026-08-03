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
    schema::Schema,
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

/// An `application/x-www-form-urlencoded` request body.
#[cfg(feature = "form")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Form<T>(pub T);

/// A `multipart/form-data` request body with declared fields.
///
/// `T` derives `Schema`, and each field becomes a part with its own `Encoding`.
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
}

/// A group of query parameters.
pub trait QueryParams: Sized + Schema {}

/// A group of request headers.
pub trait HeaderParams: Sized {
    /// The header names this group declares.
    const NAMES: &'static [&'static str];
}

/// A group of request cookies.
pub trait CookieParams: Sized {
    /// The cookie names this group declares.
    const NAMES: &'static [&'static str];
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
        let _ = operation;
        todo!()
    }
}
