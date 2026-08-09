//! Media type names used often enough to be worth naming.
//!
//! These are plain string constants rather than `mime::Mime` values because the
//! document model must be able to carry media type *ranges* and vendor types
//! that a parsed `Mime` would normalize.

/// `application/json`.
pub const APPLICATION_JSON: &str = "application/json";
/// `application/problem+json`, the RFC 9457 error format.
pub const APPLICATION_PROBLEM_JSON: &str = "application/problem+json";
/// `application/x-www-form-urlencoded`.
pub const APPLICATION_FORM_URLENCODED: &str = "application/x-www-form-urlencoded";
/// `application/octet-stream`.
pub const APPLICATION_OCTET_STREAM: &str = "application/octet-stream";
/// `multipart/form-data`.
pub const MULTIPART_FORM_DATA: &str = "multipart/form-data";
/// `text/plain`.
pub const TEXT_PLAIN: &str = "text/plain";
/// `text/event-stream`, the Server-Sent Events format.
pub const TEXT_EVENT_STREAM: &str = "text/event-stream";
/// `application/x-ndjson`, newline-delimited JSON.
pub const APPLICATION_NDJSON: &str = "application/x-ndjson";
/// `application/json-seq`, RFC 7464 JSON text sequences.
pub const APPLICATION_JSON_SEQ: &str = "application/json-seq";
