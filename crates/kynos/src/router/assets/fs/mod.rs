//! Serving a directory whose membership is not fixed.
//!
//! # Why this is behind `unchecked`
//!
//! A directory that anything may add a file to matches a set of paths no path
//! template describes ([anti-pattern
//! 3](https://github.com/getkono/kynos#anti-patterns)). So the route is
//! *recorded* instead: an entry in `x-kynos-opaque-routes` at the document
//! root, with **no `paths` key**, so a client generator emits nothing for it.
//!
//! Kynos still owns the traversal defence, the media types, the entity tags
//! and the conditional requests; only the `paths` entry is given up.
//!
//! [`assets!`](crate::assets) is the one to reach for first: an embedded set is
//! enumerable, so it is described, and nothing is waived at all.

use std::path::{Component, Path, PathBuf};

use bytes::Bytes;
use kynos_openapi::{OpaqueReason, OpaqueRoute};

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams, UnencodableHeader},
    http::{HeaderValue, Request, Response, StatusCode, header},
    middleware::catch_panic::PanicPolicy,
    response::range::{
        Selection,
        source::{ByteSource, Spans},
        spec,
    },
    router::{
        Router,
        assets::{media, range},
    },
};

/// A directory served from disk.
#[derive(Clone, Debug)]
pub struct Directory {
    root: PathBuf,
    cache_control: Option<HeaderValue>,
    index: Option<&'static str>,
}

impl Directory {
    /// Serves the files under `root`.
    ///
    /// The path is resolved once, here, so a relative one is relative to the
    /// process's working directory at build time rather than at request time.
    /// Resolution is lexical, as [`std::path::absolute`]: no link is followed
    /// and the root need not exist yet. Where it cannot be made absolute (an
    /// empty path, or a working directory that cannot be read) it is kept as
    /// given.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            root: std::path::absolute(&root).unwrap_or(root),
            cache_control: Some(super::default_cache_control()),
            index: Some("index.html"),
        }
    }

    /// The `Cache-Control` every file carries.
    ///
    /// # Panics
    ///
    /// If `value` is not a field value (RFC 9110 section 5.5), such as one
    /// holding a control character. It is configuration, so it is refused here
    /// rather than left off every file served.
    #[must_use]
    #[track_caller]
    pub fn cache_control(mut self, value: &'static str) -> Self {
        self.cache_control = Some(super::cache_control(value));
        self
    }

    /// `public, max-age=31536000, immutable`, for a fingerprinted directory.
    #[must_use]
    pub fn immutable(mut self) -> Self {
        self.cache_control = Some(super::immutable_cache_control());
        self
    }

    /// Sends no `Cache-Control`.
    #[must_use]
    pub fn no_cache_control(mut self) -> Self {
        self.cache_control = None;
        self
    }

    /// Serves `name` when a directory itself is requested.
    #[must_use]
    pub fn index(mut self, name: &'static str) -> Self {
        self.index = Some(name);
        self
    }

    /// Serves no directory index.
    #[must_use]
    pub fn no_index(mut self) -> Self {
        self.index = None;
        self
    }

    /// Resolves `requested` against the root, or `None` where it escapes or
    /// names something hidden.
    ///
    /// Structural rather than canonicalizing: any component that is not a plain
    /// name is refused, so `..` and root or prefix components never pass. Dot
    /// segments are refused too, so `.git` and `.env` are never served.
    fn resolve(&self, requested: &str) -> Option<PathBuf> {
        let mut resolved = self.root.clone();

        for segment in requested.split('/') {
            if segment.is_empty() || segment == "." {
                continue;
            }
            if segment.starts_with('.') {
                return None;
            }

            // Only a plain name yields exactly one `Normal`.
            let mut components = Path::new(segment).components();
            match (components.next(), components.next()) {
                (Some(Component::Normal(name)), None) => resolved.push(name),
                _ => return None,
            }
        }

        Some(resolved)
    }

    /// The file `requested` names on disk and what `stat` said of it, or
    /// `None` where nothing may be served.
    ///
    /// A directory stands for its index; every failure, `PermissionDenied`
    /// included, is `None` (a 404 leaks least). No link below the root is
    /// followed; the root itself is. A link swapped in after the check is not
    /// caught.
    async fn locate(&self, requested: &str) -> Option<(PathBuf, std::fs::Metadata)> {
        let resolved = self.resolve(requested)?;
        let mut path = self.root.clone();
        let mut below = None;
        for name in resolved.strip_prefix(&self.root).ok()? {
            path.push(name);
            below = Some(unlinked(&path).await?);
        }
        let mut metadata = match below {
            Some(metadata) => metadata,
            None => tokio::fs::metadata(&path).await.ok()?,
        };

        if metadata.is_dir() {
            path.push(self.index?);
            metadata = unlinked(&path).await?;
        }

        metadata.is_file().then_some((path, metadata))
    }
}

/// What `lstat` reports of `path`, or `None` where it is a link or cannot be
/// read.
async fn unlinked(path: &Path) -> Option<std::fs::Metadata> {
    let metadata = tokio::fs::symlink_metadata(path).await.ok()?;
    (!metadata.file_type().is_symlink()).then_some(metadata)
}

/// The media type a located file is served as, read from the file so an index
/// is typed as itself.
fn media_type(path: &Path) -> &'static str {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(media::for_path)
        .unwrap_or(media::FALLBACK)
}

/// A weak entity tag from the length and modification time, so a conditional
/// request never reads the file.
///
/// Weak, so under the strong comparison of RFC 9110 sections 13.1.5 and 13.1.1
/// every `If-Range` gets the whole file and every `If-Match` but `*` a 412.
fn etag(metadata: &std::fs::Metadata) -> Option<String> {
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;

    Some(format!(
        "W/\"{:x}-{:x}\"",
        metadata.len(),
        modified.as_nanos()
    ))
}

/// The `ETag` and `Cache-Control` a served file carries.
#[derive(Clone, Debug)]
struct FileHeaders {
    etag: Option<String>,
    cache_control: Option<HeaderValue>,
}

impl HeaderParams for FileHeaders {
    const NAMES: &'static [&'static str] = &["etag", "cache-control"];
}

impl EncodeHeaders for FileHeaders {
    fn encode(&self) -> Result<Vec<(crate::http::HeaderName, HeaderValue)>, UnencodableHeader> {
        let mut fields = Vec::with_capacity(2);

        if let Some(etag) = self
            .etag
            .as_deref()
            .and_then(|etag| HeaderValue::from_str(etag).ok())
        {
            fields.push((header::ETAG, etag));
        }
        if let Some(value) = &self.cache_control {
            fields.push((header::CACHE_CONTROL, value.clone()));
        }

        Ok(fields)
    }
}

/// Serves one request against the directory.
///
/// A capture that is missing or does not decode to UTF-8 names no file, so it
/// is a 404 rather than the empty path, which is the root and its index.
async fn serve(directory: &Directory, request: &Request) -> Response {
    let Some(requested) = crate::unchecked::captured(request, "path") else {
        return crate::http::body::bodiless(StatusCode::NOT_FOUND);
    };

    let Some((path, metadata)) = directory.locate(&requested).await else {
        return crate::http::body::bodiless(StatusCode::NOT_FOUND);
    };

    let headers = FileHeaders {
        etag: etag(&metadata),
        cache_control: directory.cache_control.clone(),
    };

    // Section 13.2.2 step 1, which a weak tag passes only as `*`: see `etag`.
    if let Some(refused) = range::precondition_failed(request.headers(), headers.etag.as_deref()) {
        return refused;
    }

    // Section 13.1.2, over every field line. The file was located, so a
    // tagless one is still named by `*`.
    if crate::http::etag::if_none_match(request.headers(), || headers.etag.as_deref()) == Some(true)
    {
        let mut response = crate::http::body::bodiless(StatusCode::NOT_MODIFIED);
        crate::response::headers::write_or_refuse(&mut response, &headers);
        return response;
    }

    // Section 14.2: `Range` is evaluated only once the 412 and 304 are ruled out.
    let range_set = spec::read(request.method(), request.headers(), headers.etag.as_deref());

    // Decided from `stat`'s length, so an unsatisfiable range costs no read.
    let selection = match crate::response::range::select(&range_set, metadata.len()) {
        Ok(selection) => selection,
        Err(rejection) => return range::unsatisfiable(rejection),
    };

    // Opened here, so a file that cannot be opened is still a 404 rather than
    // a head followed by a failed body. Opening reads nothing: the body reads
    // once polled, so a HEAD, whose body is dropped (section 9.3.2), reads
    // none of the file. Just opened, so its cursor is at 0.
    let Ok(file) = tokio::fs::File::open(&path).await else {
        return crate::http::body::bodiless(StatusCode::NOT_FOUND);
    };
    let source = std::sync::Arc::new(Opened(tokio::sync::Mutex::new((file, 0))));

    // An empty file has no last offset, so it gets an empty body.
    let body = match selection {
        Selection::Whole(_) => metadata.len().checked_sub(1).map(|last| (0, last)),
        Selection::Part { first, last, .. } => Some((first, last)),
    }
    .map_or_else(crate::http::body::Body::empty, |(first, last)| {
        crate::http::body::Body::from_body(Spans::new(source, first, last))
    });

    range::assembled(body, selection, media_type(&path), &headers)
}

/// A located file, read one [`SPAN`](crate::response::range::source::SPAN)
/// at a time as its body is polled.
///
/// Held open for the whole response, so a file renamed over mid-response is
/// not spliced into it. One that shrank since `stat` fails the body with
/// [`Truncated`](crate::response::range::source::Truncated) rather than ending
/// short of the length the head named (RFC 9110 section 14.4).
///
/// Holds the file and the offset its cursor is at, so a read continuing where
/// the last one stopped does not seek. A mutex only because `read_span` takes
/// `&self`: `Spans` keeps one read in flight, so it is never contended.
#[derive(Debug)]
struct Opened(tokio::sync::Mutex<(tokio::fs::File, u64)>);

impl ByteSource for Opened {
    type Error = std::io::Error;

    /// What `fstat` says; [`serve`] sizes the body from its `stat`, so never asks.
    async fn complete_length(&self) -> Result<u64, Self::Error> {
        Ok(self.0.lock().await.0.metadata().await?.len())
    }

    async fn read_span(&self, first: u64, last: u64) -> Result<Bytes, Self::Error> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let mut guard = self.0.lock().await;
        let (file, cursor) = &mut *guard;

        if *cursor != first {
            file.seek(std::io::SeekFrom::Start(first)).await?;
            *cursor = first;
        }

        // `first <= last`, so this cannot wrap; `Spans` asks for one span at
        // most, so it is never more than that.
        let length = usize::try_from(last - first + 1).unwrap_or(usize::MAX);
        let mut buffer = bytes::BytesMut::with_capacity(length);
        let read = file.read_buf(&mut buffer).await?;
        *cursor = cursor.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));

        Ok(buffer.freeze())
    }
}

impl<C: Send + Sync + 'static, P: PanicPolicy, I, S> Router<C, P, I, S> {
    /// Serves a directory from disk, under `prefix`.
    ///
    /// The route is **not** described. It is recorded under
    /// `x-kynos-opaque-routes` with [`OpaqueReason::StaticAssets`] and gets no
    /// `paths` key, so a client generator emits nothing for it. The document is
    /// stamped non-authoritative, and
    /// [`unchecked_reasons`](Router::unchecked_reasons) is how a CI gate
    /// tolerates exactly this waiver and no other.
    ///
    /// Reach for [`assets!`](crate::assets) first. An embedded set is
    /// enumerable, so it is described in full and waives nothing.
    ///
    /// ```no_run
    /// use kynos::{Router, router::assets::fs::Directory};
    ///
    /// let router = Router::<()>::new()
    ///     .assets_directory("/static", Directory::new("./public"));
    /// # let _ = router;
    /// ```
    ///
    /// A `prefix` carrying a variable is recorded as a violation and surfaces
    /// from [`Router::validate`](crate::router::Router::validate).
    #[must_use]
    pub fn assets_directory(mut self, prefix: &str, directory: Directory) -> Self {
        let prefix = prefix.trim_end_matches('/');
        if prefix.contains('{') {
            self.violations.push(kynos_openapi::Violation {
                location: "#/paths".to_owned(),
                severity: kynos_openapi::Severity::Error,
                error: kynos_openapi::SpecError::OpaqueRoute {
                    pattern: format!("{prefix}/{{*path}}"),
                }
                .into(),
            });
            return self;
        }

        let pattern = format!("{prefix}/{{*path}}");
        let record = OpaqueRoute::new(pattern.clone(), OpaqueReason::StaticAssets)
            .with_methods(["GET"])
            .with_prefix(prefix.to_owned())
            .with_note(format!(
                "a directory served from `{}`; its membership is not fixed, so no path template \
                 is true of it",
                directory.root.display()
            ));

        self.record_unchecked_route(pattern, record, move |request| {
            let directory = directory.clone();
            async move { serve(&directory, &request).await }
        })
    }
}

#[cfg(test)]
mod tests;
