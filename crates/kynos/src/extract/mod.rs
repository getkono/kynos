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
//! [`MatchedPath`](connection::MatchedPath) and
//! [`ConnectInfo`](connection::ConnectInfo) all do: none of them is visible to
//! a consumer, and each is making that claim rather than skipping a step.
//!
//! Tools that infer a description from axum handlers produce documents with
//! silent holes because an extractor there need not describe itself at all.
//! The difference is not that Kynos sorts arguments into kinds — it is that
//! the describing half is not optional.
//!
//! A consequence worth stating plainly: **there is no extractor that yields the
//! whole request**. No `Request`, no `Body`, no `HeaderMap`. Those are the
//! holes. A handler that wants an arbitrary header declares it with
//! [`Headers`](params::header::Headers); a handler that wants an arbitrary body
//! says [`Unchecked`](crate::schema::unchecked::Unchecked).
//!
//! # Rejections describe themselves too
//!
//! [`FromRequestParts::Rejection`] is bound by
//! [`Responses`](crate::response::Responses), so every way an extractor can
//! fail appears in the operation's `responses`.
//!
//! # How this module is laid out
//!
//! [`params`] holds inputs read from the request head, one module per
//! parameter location. [`body`] holds inputs that consume the body, one module
//! per codec. [`media`] names media types in the type system, and
//! [`connection`] holds the two inputs that describe the connection rather than
//! the contract.

pub mod body;
pub mod connection;
pub mod describe;
pub mod media;
pub mod params;
// The receive half of `response::stream::sse`, gated with it. A 3.1 build has
// no `itemSchema` to describe a stream with, so it has no `Sse<T>` to resume --
// and an extractor for the header a resume carries would be half a feature.
#[cfg(feature = "openapi32")]
pub mod sse;

use std::future::Future;

use crate::http::{Parts, Request};

/// A handler input built from the request head.
///
/// Every implementation must also implement [`Describe`](describe::Describe);
/// the two are separate traits only because the runtime half is generic over
/// the application context and the describing half is not.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be extracted from a request head",
    label = "not extractable",
    note = "the context type `{C}` may be the problem rather than `{Self}`: `Inject<T>` needs \
            `{C}: Provides<T>`, and `Auth<S>` needs `{C}: Authenticates<S>`"
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
/// At most one argument per handler may implement this, and it must be the
/// last. The split from [`FromRequestParts`] is what enforces that: the body
/// can only be taken once, so the type system takes it once.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be extracted from a request body",
    label = "not a body extractor",
    note = "only the last handler argument consumes the body; every earlier one reads the head"
)]
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
