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
//! Kynos does define, and the erasure behind it, and [`cookie`] the one field
//! whose grammar needs reading rather than looking up.

pub mod body;
pub mod cookie;

use crate::http::body::Body;

#[doc(no_inline)]
pub use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version, header};

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
