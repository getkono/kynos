//! The streamed JSON codecs: newline-delimited, and RFC 7464 text sequences.

/// A newline-delimited JSON response (`application/x-ndjson`).
///
/// Requires both `json` and `openapi32`; the latter supplies the `itemSchema`
/// needed to describe each streamed value.
///
/// ```no_run
/// # #[cfg(all(feature = "json", feature = "openapi32"))]
/// # {
/// use kynos::response::stream::json::JsonLines;
///
/// fn lines<S>(items: S) -> JsonLines<S> {
///     JsonLines { items }
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct JsonLines<S> {
    /// The stream of items.
    pub items: S,
}

/// An RFC 7464 JSON text sequence response (`application/json-seq`).
///
/// Requires both `json` and `openapi32`; the latter supplies the `itemSchema`
/// needed to describe each streamed value.
///
/// ```no_run
/// # #[cfg(all(feature = "json", feature = "openapi32"))]
/// # {
/// use kynos::response::stream::json::JsonSeq;
///
/// fn sequence<S>(items: S) -> JsonSeq<S> {
///     JsonSeq { items }
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct JsonSeq<S> {
    /// The stream of items.
    pub items: S,
}
