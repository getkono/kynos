//! Responses delivered as a sequence of chunks or items rather than one value.
//!
//! [`binary`] is in every build: a streamed binary body is described as the
//! whole representation, exactly as a buffered one, which OpenAPI 3.1 already
//! can. `sse` and JSON Lines require `openapi32`, since only 3.2's
//! `itemSchema` describes each item of a sequential media type.

pub mod binary;
#[cfg(feature = "openapi32")]
pub mod sse;

// Private: it only implements for the types `extract::body::json_lines` declares.
#[cfg(all(feature = "json", feature = "openapi32"))]
mod json;
