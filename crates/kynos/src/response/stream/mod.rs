//! Responses delivered as a sequence of items rather than one value.
//!
//! Every module here requires `openapi32`: OpenAPI 3.1 can describe a stream
//! only as an opaque string, while 3.2's `itemSchema` describes each item.

pub mod binary;
pub mod sse;

// Private: it only implements for the types `extract::body::json_lines` declares.
#[cfg(feature = "json")]
mod json;
