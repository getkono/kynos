//! The HTTP types Kynos builds on.
//!
//! Kynos uses the `http` crate's request and response types, so an application
//! composes with the rest of the ecosystem. No extractor yields a whole
//! [`Request`]: a handler that reads an arbitrary part of the request cannot
//! describe what it read.
//!
//! [`body`] holds the one type Kynos defines; [`cookie`] and [`etag`] read
//! their fields' grammar; [`forwarded`] resolves the client behind trusted
//! proxies; [`media`] names media types in the type system.

pub mod body;
pub mod cookie;
pub mod etag;
pub mod forwarded;
pub mod media;

// Private: a handler only ever sees a coding already chosen.
#[cfg(any(feature = "compression", feature = "assets"))]
pub(crate) mod coding;
// Private: a handler passes a `SystemTime`; only responses render and read dates.
pub(crate) mod date;
// Private: a weight reaches a handler already folded into the winning choice.
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

/// RFC 9110 section 5.6.2 `token`, shared by `cookie` and `assets`.
#[cfg(any(feature = "cookie", feature = "assets"))]
pub(crate) fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}
