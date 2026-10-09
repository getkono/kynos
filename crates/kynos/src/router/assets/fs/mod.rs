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
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderValue, Request, Response, StatusCode, header},
    middleware::catch_panic::PanicPolicy,
    response::range::{Selection, spec},
    router::{
        Router,
        assets::{media, range},
    },
};

/// A directory served from disk.
#[derive(Clone, Debug)]
pub struct Directory {
    root: PathBuf,
    cache_control: Option<&'static str>,
    index: Option<&'static str>,
}

impl Directory {
    /// Serves the files under `root`.
    ///
    /// The path is resolved once, here, so a relative one is relative to the
    /// process's working directory at build time rather than at request time.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cache_control: Some(super::DEFAULT_CACHE_CONTROL),
            index: Some("index.html"),
        }
    }

    /// The `Cache-Control` every file carries.
    #[must_use]
    pub fn cache_control(mut self, value: &'static str) -> Self {
        self.cache_control = Some(value);
        self
    }

    /// `public, max-age=31536000, immutable`, for a fingerprinted directory.
    #[must_use]
    pub fn immutable(mut self) -> Self {
        self.cache_control = Some("public, max-age=31536000, immutable");
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
    cache_control: Option<&'static str>,
}

impl HeaderParams for FileHeaders {
    const NAMES: &'static [&'static str] = &["etag", "cache-control"];
}

impl EncodeHeaders for FileHeaders {
    fn encode(&self) -> Vec<(crate::http::HeaderName, HeaderValue)> {
        let mut fields = Vec::with_capacity(2);

        if let Some(etag) = self
            .etag
            .as_deref()
            .and_then(|etag| HeaderValue::from_str(etag).ok())
        {
            fields.push((header::ETAG, etag));
        }
        if let Some(value) = self
            .cache_control
            .and_then(|value| HeaderValue::from_str(value).ok())
        {
            fields.push((header::CACHE_CONTROL, value));
        }

        fields
    }
}

/// Serves one request against the directory.
async fn serve(directory: &Directory, request: &Request) -> Response {
    let requested = crate::unchecked::captured(request, "path").unwrap_or_default();

    let Some((path, metadata)) = directory.locate(&requested).await else {
        return refused(StatusCode::NOT_FOUND);
    };

    let headers = FileHeaders {
        etag: etag(&metadata),
        cache_control: directory.cache_control,
    };

    // Section 13.2.2 step 1, which a weak tag passes only as `*`: see `etag`.
    if let Some(refused) = range::precondition_failed(request.headers(), headers.etag.as_deref()) {
        return refused;
    }

    if let (Some(tag), Some(field)) = (
        headers.etag.as_deref(),
        request.headers().get(header::IF_NONE_MATCH),
    ) {
        if crate::http::etag::matches(field, tag) {
            let mut response = Response::new(crate::http::body::Body::empty());
            *response.status_mut() = StatusCode::NOT_MODIFIED;
            crate::extract::params::header::write(response.headers_mut(), &headers);
            return response;
        }
    }

    // Section 14.2: `Range` is evaluated only once the 412 and 304 are ruled out.
    let range_set = spec::read(request.method(), request.headers(), headers.etag.as_deref());

    // Decided from `stat`'s length, so an unsatisfiable range costs no read.
    let selection = match crate::response::range::select(&range_set, metadata.len()) {
        Ok(selection) => selection,
        Err(rejection) => return range::unsatisfiable(rejection),
    };

    let read = match selection {
        Selection::Whole(_) => tokio::fs::read(&path).await.map(Bytes::from),
        Selection::Part { first, last, .. } => span(&path, first, last).await,
    };
    let Ok(body) = read else {
        return refused(StatusCode::NOT_FOUND);
    };

    range::assembled(body, selection, media_type(&path), &headers)
}

/// The bytes from `first` to `last` inclusive, without reading the rest.
///
/// `read_exact`, so a file that shrank since `stat` is a failed read rather
/// than fewer octets than `Content-Range` names (RFC 9110 section 14.4).
async fn span(path: &Path, first: u64, last: u64) -> std::io::Result<Bytes> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = tokio::fs::File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(first)).await?;

    // `first <= last < length`, so this cannot wrap.
    let length = usize::try_from(last - first + 1).unwrap_or(usize::MAX);
    let mut buffer = vec![0_u8; length];
    file.read_exact(&mut buffer).await?;

    Ok(Bytes::from(buffer))
}

/// An empty response with `status`.
fn refused(status: StatusCode) -> Response {
    let mut response = Response::new(crate::http::body::Body::empty());
    *response.status_mut() = status;
    response
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
                },
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
