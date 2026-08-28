//! Serving a directory whose membership is not fixed.
//!
//! # Why this is behind `unchecked`
//!
//! A directory that anything may add a file to matches a set of paths no single
//! path template describes. `/static/{*path}` is not a template: a path
//! parameter's value must not contain an unescaped `/`, so every key that could
//! be minted is a claim about either the path or a parameter the service does
//! not honour. [Anti-pattern 3](https://github.com/getkono/kynos#anti-patterns)
//! is right about it, and no amount of care makes it describable.
//!
//! So the route is *recorded* instead: an entry in `x-kynos-opaque-routes` at
//! the document root, with **no `paths` key**. A client generator emits nothing
//! for it by construction rather than by convention — which is stronger than a
//! `paths` entry marked with a vendor extension a generator may or may not
//! honour.
//!
//! Waiving the description does not waive the implementation. Kynos still owns
//! the traversal defence, the media types, the entity tags and the conditional
//! requests. What is given up is the `paths` entry, not the correctness.
//!
//! [`assets!`](crate::assets) is the other half, and the one to reach for
//! first: an embedded set is enumerable, so it is described, and nothing is
//! waived at all.

use std::path::{Component, Path, PathBuf};

use bytes::Bytes;
use kynos_openapi::{OpaqueReason, OpaqueRoute};

use crate::{
    extract::params::header::HeaderParams,
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

    /// Resolves `requested` against the root, or `None` where it escapes.
    ///
    /// Structural rather than canonicalizing. Every component is examined and
    /// anything that is not a plain name is refused: `..` cannot climb out
    /// because it is never accepted, and a root or prefix component cannot
    /// replace the base because it is never accepted either. A check that
    /// canonicalized and then compared is one that has to be remembered; this
    /// one cannot be forgotten, because there is no branch that admits the bad
    /// input.
    fn resolve(&self, requested: &str) -> Option<PathBuf> {
        let mut resolved = self.root.clone();

        for segment in requested.split('/') {
            if segment.is_empty() || segment == "." {
                continue;
            }

            // `Path::new(segment).components()` is what turns a segment into a
            // verdict: a plain name yields exactly one `Normal`, and anything
            // else — `..`, `/`, a Windows prefix — yields something else.
            let mut components = Path::new(segment).components();
            match (components.next(), components.next()) {
                (Some(Component::Normal(name)), None) => resolved.push(name),
                _ => return None,
            }
        }

        Some(resolved)
    }
}

/// A weak entity tag from what a `stat` already knows.
///
/// Weak, and that is the honest strength: it is derived from the length and the
/// modification time rather than from the contents, so two different files
/// written in the same nanosecond with the same length would share it. Reading
/// every file to hash it would turn a conditional request into the work it
/// exists to avoid.
///
/// # So an `If-Range` is never honoured here
///
/// RFC 9110 section 13.1.5 evaluates that condition with the *strong*
/// comparison, under which a weak tag is equivalent to nothing — not even to
/// itself. A directory therefore answers every `If-Range` request with the
/// whole file and a 200, which is the correct answer rather than a missing
/// feature: a client splicing a part into a copy it holds needs to know the
/// representation has not changed, and a tag this one cannot promise that.
/// A plain `Range` with no condition is served as a 206 exactly as an embedded
/// file's is; it is only the precondition that a weak validator cannot pass.
/// [`assets!`](crate::assets) hashes the contents and gets a strong tag, which
/// is the mode to reach for when resumption matters.
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

    let Some(mut path) = directory.resolve(&requested) else {
        return refused(StatusCode::NOT_FOUND);
    };

    // Every read failure is a 404, including `PermissionDenied`. A file the
    // process cannot read is, to a client, not there — and 404 leaks least.
    // The same holds for the index read below, which stays a `match` only
    // because it reassigns rather than binds.
    let Ok(mut metadata) = tokio::fs::metadata(&path).await else {
        return refused(StatusCode::NOT_FOUND);
    };

    if metadata.is_dir() {
        let Some(index) = directory.index else {
            return refused(StatusCode::NOT_FOUND);
        };
        path.push(index);
        metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(_) => return refused(StatusCode::NOT_FOUND),
        };
    }

    if !metadata.is_file() {
        return refused(StatusCode::NOT_FOUND);
    }

    let headers = FileHeaders {
        etag: etag(&metadata),
        cache_control: directory.cache_control,
    };

    if let (Some(tag), Some(field)) = (
        headers.etag.as_deref(),
        request.headers().get(header::IF_NONE_MATCH),
    ) {
        if super::endpoint::matches(field, tag) {
            let mut response = Response::new(crate::http::body::Body::empty());
            *response.status_mut() = StatusCode::NOT_MODIFIED;
            crate::extract::params::header::write(response.headers_mut(), &headers);
            return response;
        }
    }

    // Section 14.2: the `Range` field is evaluated *only if the result in
    // absence of the Range header field would be a 200*, which the 304 above
    // has already settled. The validator goes with it and is weak, so section
    // 13.1.5's condition never holds -- see `etag` for why that is the answer
    // rather than a gap.
    let range_set = spec::read(request.method(), request.headers(), headers.etag.as_deref());

    // `stat` already reported the length, so satisfiability is decided before a
    // byte is read -- and an unsatisfiable field costs no read at all.
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

    let media_type = media::for_path(&requested).unwrap_or(media::FALLBACK);
    range::assembled(body, selection, media_type, &headers)
}

/// The bytes from `first` to `last` inclusive, without reading the rest.
///
/// A seek and one sized read rather than `tokio::fs::read` and a slice. Serving
/// a kilobyte out of a gigabyte should cost a kilobyte, and that difference is
/// most of the reason a range request exists at all — slicing after the read
/// would be honest about the octets and wrong about the work.
///
/// `read_exact` rather than a read that settles for less: the file may have
/// changed since the `stat` that fixed the length, and sending fewer octets
/// than the `Content-Range` names produces a field RFC 9110 section 14.4 tells
/// a recipient never to recombine. A file that shrank underneath the request is
/// a failed read, which the caller answers the way it answers every other one.
async fn span(path: &Path, first: u64, last: u64) -> std::io::Result<Bytes> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = tokio::fs::File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(first)).await?;

    // `last` came from the resolver, so it is at most one less than the length
    // `stat` reported and neither the subtraction nor the addition can wrap.
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

impl<C: Send + Sync + 'static, P: PanicPolicy, I> Router<C, P, I> {
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
    /// from [`Router::validate`](crate::router::Router::validate): it is the
    /// mount point rather than a template, so it has no variables of its own.
    /// This used to `assert!`, which made a path literal at a mount site the
    /// one kind of malformed path that stopped the program instead of being
    /// reported with the rest.
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
