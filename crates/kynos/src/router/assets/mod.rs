//! Serving files, and describing what is served.
//!
//! # Two things share this name
//!
//! **A known set of files** — a build output: `app.js`, `app.css`,
//! `index.html`, fonts. Finite, enumerable, and stable for the life of the
//! process. Every path is a literal, so there is no wildcard and nothing to
//! waive: [`assets!`](crate::assets) compiles the set into the binary and each
//! file becomes an ordinary described operation.
//!
//! **A directory as a live namespace** — drop a file in and it serves. That
//! genuinely matches a set of paths no template describes, and
//! [anti-pattern 3](https://github.com/getkono/kynos#anti-patterns) is right
//! about it. `Router::assets_directory` serves one behind the `unchecked`
//! feature, recorded at the document root where no client generator can act on
//! it.
//!
//! Compile-time embedding is not a limitation of the first case. It is what
//! *makes* the set knowable, which is what makes it describable.
//!
//! # What is described
//!
//! An embedded file is one `paths` key: a 200 with its media type, a 304, and
//! an `ETag`. There is no `Last-Modified` and no `If-Modified-Since`, which is
//! a decision rather than an omission — a strong entity tag is the stronger
//! validator, and sending a date obliges honouring a request that carries one
//! back. Sending neither half is consistent; sending one is not.

pub mod media;

use std::borrow::Cow;

use crate::router::endpoint::set::{Endpoints, IntoEndpoints};

mod endpoint;

pub use endpoint::AssetEndpoint;

/// One file an asset set serves.
///
/// `const`-constructible throughout, so an embedded set is one `static` and
/// costs nothing to hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asset {
    path: &'static str,
    bytes: &'static [u8],
    etag: &'static str,
}

impl Asset {
    /// A file compiled into the binary.
    ///
    /// What [`assets!`](crate::assets) emits. `path` is relative and
    /// `/`-separated with no leading slash; `etag` is quoted and ready for the
    /// field.
    #[must_use]
    pub const fn embedded(path: &'static str, bytes: &'static [u8], etag: &'static str) -> Self {
        Self { path, bytes, etag }
    }

    /// The path, relative to wherever the set is mounted.
    #[must_use]
    pub const fn path(&self) -> &'static str {
        self.path
    }

    /// The bytes.
    #[must_use]
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The entity tag, quoted.
    #[must_use]
    pub const fn etag(&self) -> &'static str {
        self.etag
    }

    /// The media type, from the table.
    #[must_use]
    pub fn media_type(&self) -> &'static str {
        media::for_path(self.path).unwrap_or(media::FALLBACK)
    }

    /// The byte length.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the file is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// A set of files, ready to mount.
///
/// Mounted the way anything else is. There is no `Router::assets`, because a
/// [`Group`](crate::router::group::Group) already supplies the prefix, the tag
/// and the interceptors — and an asset set that needed its own mounting verb
/// would be one the router did not really accept.
///
/// ```no_run
/// # use kynos::{Router, router::group::Group};
/// # fn set() -> kynos::router::assets::AssetSet { todo!() }
/// let router = Router::<()>::new().group(Group::new("/static").mount(set()));
/// # let _ = router;
/// ```
#[derive(Clone, Debug)]
pub struct AssetSet {
    assets: &'static [Asset],
    cache_control: Option<&'static str>,
    index: Option<&'static str>,
    operation_id_prefix: Cow<'static, str>,
}

/// What a set carrying no `Cache-Control` of its own sends.
///
/// An hour: long enough to be worth a cache, short enough that a deployment
/// which forgot to fingerprint its files is not stuck for a year.
/// [`immutable`](AssetSet::immutable) is the fingerprinted answer.
const DEFAULT_CACHE_CONTROL: &str = "public, max-age=3600";

impl AssetSet {
    /// A set over files compiled into the binary.
    #[must_use]
    pub fn embedded(assets: &'static [Asset]) -> Self {
        Self {
            assets,
            cache_control: Some(DEFAULT_CACHE_CONTROL),
            index: Some("index.html"),
            operation_id_prefix: Cow::Borrowed("asset"),
        }
    }

    /// Serves `name` at each directory's own path as well as at its own.
    ///
    /// `index.html` by default, because a set that serves it at
    /// `/index.html` and 404s at `/` surprises everyone. Both URLs are served,
    /// so both are described.
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

    /// The `Cache-Control` every asset carries.
    #[must_use]
    pub fn cache_control(mut self, value: &'static str) -> Self {
        self.cache_control = Some(value);
        self
    }

    /// `public, max-age=31536000, immutable`, for a fingerprinted set.
    ///
    /// Say this deliberately rather than having Kynos guess from the file
    /// names: a set that is *not* fingerprinted and claims to be is cached for
    /// a year by every client that saw it, and there is no way to take it back.
    #[must_use]
    pub fn immutable(mut self) -> Self {
        self.cache_control = Some("public, max-age=31536000, immutable");
        self
    }

    /// Sends no `Cache-Control` at all, leaving the decision to whatever is in
    /// front.
    #[must_use]
    pub fn no_cache_control(mut self) -> Self {
        self.cache_control = None;
        self
    }

    /// The prefix every `operationId` takes.
    #[must_use]
    pub fn operation_id_prefix(mut self, prefix: impl Into<Cow<'static, str>>) -> Self {
        self.operation_id_prefix = prefix.into();
        self
    }

    /// How many operations this set registers.
    ///
    /// One per file, plus one per directory index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.assets.len() + self.indexed().count()
    }

    /// Whether the set serves nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The files, and the directory path each index is also served at.
    fn indexed(&self) -> impl Iterator<Item = (&'static Asset, String)> + '_ {
        self.assets.iter().filter_map(move |asset| {
            let index = self.index?;
            let directory = asset.path.strip_suffix(index)?;
            // `index.html` at the root serves `/`; `docs/index.html` serves
            // `docs/`. Both keep the trailing slash, which is what a browser
            // resolving a relative link against them expects.
            Some((asset, directory.to_owned()))
        })
    }
}

impl<C: Send + Sync + 'static> IntoEndpoints<C> for AssetSet {
    /// An asset carries no interceptors of its own, so there is no stack to
    /// check at the mount site.
    type Stacks = ();

    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        for asset in self.assets {
            sink.push(AssetEndpoint::new(
                *asset,
                asset.path.to_owned(),
                self.cache_control,
                &self.operation_id_prefix,
            ));
        }

        for (asset, directory) in self.indexed() {
            sink.push(AssetEndpoint::new(
                *asset,
                directory,
                self.cache_control,
                &self.operation_id_prefix,
            ));
        }
    }
}

#[cfg(test)]
mod tests;
