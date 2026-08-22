//! Which extension carries which media type.
//!
//! A table Kynos writes down rather than a database it depends on. The whole
//! set is here, which makes it a *closed enumeration* under `docs/testing.md`:
//! one test asserts it is closed and fails when a row is added without one.
//! `mime_guess` bundles a generated database that only sampling can check, and
//! [`mime_names`](kynos_openapi::model::body::mime_names) already records why a
//! media type is a `&'static str` here rather than a parsed type.
//!
//! A row is not a claim about a file's contents. It is what a service *says* a
//! file is, and the emitted description prints it — so a wrong row is visible
//! rather than silent.

/// Every extension the built-in table names.
///
/// Longest suffix wins, so `.tar.gz` would beat `.gz` if both were listed.
/// Sorted by extension, because a table a human maintains is one a human reads.
pub const EXTENSIONS: &[(&str, &str)] = &[
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
    (".json", "application/json"),
    (".map", "application/json"),
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

/// What an extension the table does not name serves as.
///
/// RFC 9110 section 8.3: a sender that does not know the media type sends this
/// rather than guessing, and a recipient treats it as an opaque stream. Which
/// is the honest answer, and better than a guess a browser might act on.
pub const FALLBACK: &str = "application/octet-stream";

/// The media type `path`'s extension names, or `None`.
///
/// The longest matching suffix wins, and the comparison is
/// ASCII-case-insensitive because a file called `LOGO.PNG` is a PNG.
#[must_use]
pub fn for_path(path: &str) -> Option<&'static str> {
    let lowered = path.to_ascii_lowercase();

    EXTENSIONS
        .iter()
        .filter(|(extension, _)| lowered.ends_with(extension))
        .max_by_key(|(extension, _)| extension.len())
        .map(|(_, media_type)| *media_type)
}
