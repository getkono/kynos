//! Responses delivered as a sequence of items rather than one value.
//!
//! Every module here requires `openapi32`. Under OpenAPI 3.1 a stream can only
//! be described as an opaque string, which says nothing useful about what it
//! carries; 3.2's `itemSchema` is what makes the items describable. Kynos would
//! rather not compile than emit a description that lies about a stream — which
//! is why the gate is on this whole subtree rather than on each impl.

pub mod binary;
pub mod sse;

// Private, unlike its siblings: `binary` declares `BinaryStream` and `sse`
// declares `Sse` and its parts, where this module only implements for the two
// types `extract::body::json_lines` declares.
#[cfg(feature = "json")]
mod json;
