//! The HTTP types Kynos builds on.
//!
//! Kynos does not define its own request or response types. It uses the `http`
//! crate's, which the whole Rust HTTP ecosystem shares, so that a Kynos
//! application composes with anything else that speaks them.
//!
//! What Kynos *does* withhold is access to them from a handler: there is no
//! extractor yielding a whole [`Request`], because a handler that reads an
//! arbitrary part of the request cannot describe what it read.

use bytes::Bytes;

#[doc(no_inline)]
pub use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version, header};

/// The request body.
///
/// Opaque by design. Bodies are consumed through a typed extractor such as
/// [`Json`](crate::extract::Json), never read directly.
#[derive(Debug)]
pub struct Body {
    _private: (),
}

impl Body {
    /// An empty body.
    #[must_use]
    pub fn empty() -> Self {
        todo!()
    }

    /// A body holding exactly these bytes.
    #[must_use]
    pub fn from_bytes(bytes: Bytes) -> Self {
        let _ = bytes;
        todo!()
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::empty()
    }
}

/// An incoming request.
pub type Request = http::Request<Body>;

/// The head of an incoming request: everything but the body.
///
/// This is what a [`FromRequestParts`](crate::extract::FromRequestParts)
/// implementation sees.
pub type Parts = http::request::Parts;

/// An outgoing response.
pub type Response = http::Response<Body>;

/// The head of an outgoing response.
pub type ResponseParts = http::response::Parts;
