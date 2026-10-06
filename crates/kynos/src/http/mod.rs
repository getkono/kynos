//! The HTTP types Kynos builds on.
//!
//! Kynos does not define its own request or response types. It uses the `http`
//! crate's, which the whole Rust HTTP ecosystem shares, so that a Kynos
//! application composes with anything else that speaks them.
//!
//! What Kynos *does* withhold is access to them from a handler: there is no
//! extractor yielding a whole [`Request`], because a handler that reads an
//! arbitrary part of the request cannot describe what it read.
//!
//! # How this module is laid out
//!
//! The request and response aliases live here; [`body`] holds the one type
//! Kynos does define, and the erasure behind it, and [`cookie`] and [`etag`]
//! the two fields whose grammar needs reading rather than looking up.
//! [`forwarded`] resolves which client sent a request through the proxies the
//! application trusts. [`media`] names media types in the type system, for
//! request and response bodies alike. The
//! qvalue grammar every `Accept*` field shares, the `Accept-Encoding` reader
//! and the HTTP-date grammar are private beside them.

pub mod body;
pub mod cookie;
pub mod etag;
pub mod forwarded;
pub mod media;

// Not `pub`, like `quality`: a content coding reaches a handler already chosen.
// Behind the two features that negotiate one, which are its only callers.
#[cfg(any(feature = "compression", feature = "assets"))]
pub(crate) mod coding;
// Not `pub`: a handler hands a ranged response a `SystemTime`, and the
// response alone renders it as `Last-Modified` and reads `If-Modified-Since`.
pub(crate) mod date;
// Not `pub`: a weight reaches a handler already folded into whichever
// alternative won, so there is no item here for a path to point at. The same
// standing `middleware::erased` has.
pub(crate) mod quality;

use crate::http::body::Body;

#[doc(no_inline)]
pub use http::{
    Extensions, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version, header,
};

/// An incoming request.
pub type Request = http::Request<Body>;

/// The head of an incoming request: everything but the body.
///
/// This is what a [`FromRequestParts`](crate::extract::FromRequestParts)
/// implementation sees.
pub type Parts = http::request::Parts;

/// An outgoing response.
pub type Response = http::Response<Body>;

/// RFC 9110 section 5.6.2 `token`.
///
/// Here rather than with either caller: a cookie name and a stored content
/// coding are both tokens, and the two sit behind different features.
#[cfg(any(feature = "cookie", feature = "assets"))]
pub(crate) fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}
