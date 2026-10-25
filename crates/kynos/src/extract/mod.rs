//! Turning a request into a handler's arguments.
//!
//! # The rule that makes Kynos work
//!
//! **Every handler argument implements [`Describe`](describe::Describe).**
//! There is no second kind of argument and no exemption, so an operation
//! cannot read something its description never mentions.
//!
//! An argument that contributes nothing says so, by implementing `Describe`
//! with an empty body. [`Inject`](crate::di::inject::Inject),
//! [`MatchedPath`](connection::MatchedPath),
//! [`ConnectInfo`](connection::ConnectInfo) and
//! [`Forwarded`](crate::http::forwarded::Forwarded) all do: none of them is
//! visible to a consumer.
//!
//! **There is no extractor that yields the whole request**: no `Request`, no
//! `Body`, no `HeaderMap`. A handler that wants an arbitrary header declares it with
//! [`Headers`](params::header::Headers); a handler that wants an arbitrary body
//! says [`Unchecked`](crate::schema::unchecked::Unchecked).
//!
//! # Rejections describe themselves too
//!
//! Both [`FromRequestParts::Rejection`] and [`FromRequest::Rejection`] are
//! bound by [`Responses`](crate::response::Responses), so every way an
//! extractor can fail — reading the head or consuming the body — appears in
//! the operation's `responses`.
//!
//! # How this module is laid out
//!
//! [`params`] holds inputs read from the request head, one module per
//! parameter location. [`body`] holds inputs that consume the body, one module
//! per codec, typed by the media type markers in
//! [`http::media`](crate::http::media). [`connection`] holds the inputs that
//! describe the connection rather than the contract — the matched path, the
//! peer address and the connection itself, with the TLS identity it carries —
//! and [`link`] the
//! input that resolves another operation's URI against the built router.

pub mod body;
pub mod connection;
pub mod describe;
pub mod link;
pub mod params;
// Gated with `response::stream::sse`, its send half.
#[cfg(feature = "openapi32")]
pub mod sse;
// Private: a handler only ever sees the value it read, never the reader.
#[cfg(any(feature = "json", feature = "openapi32"))]
pub(crate) mod strict_json;

use std::future::Future;

use crate::http::{Parts, body::Body};

/// A handler input built from the request head.
///
/// Every implementation must also implement [`Describe`](describe::Describe),
/// which is a separate trait because it is not generic over the context.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be extracted from a request head",
    label = "not extractable",
    note = "the context type `{C}` may be the problem rather than `{Self}`: `Inject<T>` needs \
            `{C}: Provides<T>`",
    note = "a guard — `Auth`, `MaybeAuth` or `Scoped` — is not extracted here: it is a \
            handler's first argument, and implements `Guard`"
)]
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
/// Only the last argument of a handler may implement this, so the body is
/// taken once; [`FromRequestParts`] reads the head.
///
/// The head is lent rather than handed over inside a whole
/// [`Request`](crate::http::Request): the endpoint's future already owns it,
/// and a request passed by value would be a copy the extractor's future keeps
/// for as long as it runs. A caller holding a whole request splits it with
/// `into_parts`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be extracted from a request body",
    label = "not a body extractor",
    note = "only the last handler argument consumes the body; every earlier one reads the head"
)]
pub trait FromRequest<C>: Sized + Send {
    /// How this extractor fails, and what that failure looks like in the
    /// description.
    type Rejection: crate::response::IntoResponse + crate::response::Responses;

    /// Consumes the body, read against the head, or explains why it could not.
    fn from_request(
        parts: &mut Parts,
        body: Body,
        context: &C,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send;
}
