//! Which extension carries which media type.
//!
//! A closed table rather than a dependency, so a test can cover every row
//! (`docs/testing.md`).

use kynos_openapi::model::body::mime_names;

/// Every extension the built-in table names.
///
/// Longest suffix wins. Kept sorted by extension.
pub(crate) const EXTENSIONS: &[(&str, &str)] = &[
    (".atom", "application/atom+xml"),
    (".avif", "image/avif"),
    (".bmp", "image/bmp"),
    (".css", "text/css; charset=utf-8"),
    (".csv", "text/csv; charset=utf-8"),
    (".eot", "application/vnd.ms-fontobject"),
    (".gif", "image/gif"),
    (".gz", "application/gzip"),
    (".htm", "text/html; charset=utf-8"),
    (".html", "text/html; charset=utf-8"),
    (".ico", "image/vnd.microsoft.icon"),
    (".jpeg", "image/jpeg"),
    (".jpg", "image/jpeg"),
    (".js", "text/javascript; charset=utf-8"),
    (".json", mime_names::APPLICATION_JSON),
    (".map", mime_names::APPLICATION_JSON),
    (".md", "text/markdown; charset=utf-8"),
    (".mjs", "text/javascript; charset=utf-8"),
    (".mp3", "audio/mpeg"),
    (".mp4", "video/mp4"),
    (".ogg", "audio/ogg"),
    (".otf", "font/otf"),
    (".pdf", "application/pdf"),
    (".png", "image/png"),
    (".rss", "application/rss+xml"),
    (".svg", "image/svg+xml"),
    (".ttf", "font/ttf"),
    (".txt", "text/plain; charset=utf-8"),
    (".wasm", "application/wasm"),
    (".wav", "audio/wav"),
    (".webm", "video/webm"),
    (".webmanifest", "application/manifest+json"),
    (".webp", "image/webp"),
    (".woff", "font/woff"),
    (".woff2", "font/woff2"),
    (".xml", "application/xml"),
    (".zip", "application/zip"),
];

/// What an extension the table does not name serves as: an opaque stream
/// rather than a guess (RFC 9110 section 8.3).
pub(crate) const FALLBACK: &str = kynos_openapi::model::body::mime_names::APPLICATION_OCTET_STREAM;

/// The media type `path`'s extension names, or `None`.
///
/// The longest matching suffix wins, compared ASCII-case-insensitively.
#[must_use]
pub(crate) fn for_path(path: &str) -> Option<&'static str> {
    let lowered = path.to_ascii_lowercase();

    EXTENSIONS
        .iter()
        .filter(|(extension, _)| lowered.ends_with(extension))
        .max_by_key(|(extension, _)| extension.len())
        .map(|(_, media_type)| *media_type)
}
