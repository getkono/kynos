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
//! # What is described
//!
//! An embedded file is one `paths` key: a 200 with its media type, a 304, a
//! 206, a 416, and the `ETag`, `Cache-Control`, `Accept-Ranges` and
//! `Content-Range` each of those carries. There is no `Last-Modified` and no
//! `If-Modified-Since`: the strong entity tag is the validator.
//!
//! # Byte ranges
//!
//! Both modes serve byte ranges (RFC 9110 section 14.1.2), through
//! [`response::range`](crate::response::range).

mod media;

use std::borrow::Cow;

use crate::router::assets::endpoint::AssetEndpoint;
use crate::router::endpoint::set::{Endpoints, IntoEndpoints};

pub mod endpoint;

pub mod error;

mod range;

#[cfg(feature = "assets-fs")]
pub mod fs;

/// One file an asset set serves.
///
/// `const`-constructible through the unchecked constructors, so an embedded set
/// is one `static` and costs nothing to hold.
/// [`try_embedded`](Asset::try_embedded) is the checked one, for an asset built
/// by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asset {
    path: &'static str,
    bytes: &'static [u8],
    etag: &'static str,
    encodings: &'static [Encoded],
}

/// One stored content coding of an asset, with a validator of its own.
///
/// A *stored* coding: a build pipeline writes `app.js.br` beside `app.js`, and
/// [`assets!`](crate::assets) folds the two into one resource. Kynos compresses
/// nothing here; these octets are hashed at compile time like any other file.
///
/// Each coding carries its own strong tag, since a tag names one
/// representation (RFC 9110 section 8.8.1) and a range is calculated over the
/// encoded octets (section 14.1.2); a shared tag would let `If-Range` splice
/// encoded octets onto an identity prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Encoded {
    coding: &'static str,
    bytes: &'static [u8],
    etag: &'static str,
}

impl Encoded {
    /// A coding of a file, stored beside it.
    ///
    /// What [`assets!`](crate::assets) emits. `coding` is the token
    /// `Content-Encoding` carries; `etag` is minted from `bytes` rather than
    /// from the file it encodes.
    ///
    /// `etag` must be a quoted `entity-tag` (RFC 9110 section 8.8.3), as
    /// `assets!` mints it: it is sent and compared as given, checked only when
    /// [`Asset::try_embedded_with_codings`] builds the asset holding it.
    #[must_use]
    pub const fn stored(coding: &'static str, bytes: &'static [u8], etag: &'static str) -> Self {
        Self {
            coding,
            bytes,
            etag,
        }
    }

    /// The coding token, as `Content-Encoding` spells it.
    #[must_use]
    pub const fn coding(&self) -> &'static str {
        self.coding
    }

    /// The stored octets.
    #[must_use]
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// The entity tag for *these* octets, quoted.
    #[must_use]
    pub const fn etag(&self) -> &'static str {
        self.etag
    }
}

impl Asset {
    /// A file compiled into the binary.
    ///
    /// What [`assets!`](crate::assets) emits. `path` is relative and
    /// `/`-separated with no leading slash; `etag` must be a quoted
    /// `entity-tag` (RFC 9110 section 8.8.3), as `assets!` mints it: it is sent
    /// and compared as given, never checked.
    ///
    /// # Panics
    ///
    /// Not here: mounting a set that holds this asset panics when `path` is not
    /// a legal path template, through [`Router::mount`](crate::Router::mount)
    /// or [`Group::mount`](crate::router::group::Group::mount). `assets!`
    /// refuses such a name at compile time, so only a hand-built `Asset` can
    /// reach it — build one with [`try_embedded`](Self::try_embedded) instead
    /// to be refused where it is made.
    #[must_use]
    pub const fn embedded(path: &'static str, bytes: &'static [u8], etag: &'static str) -> Self {
        Self {
            path,
            bytes,
            etag,
            encodings: &[],
        }
    }

    /// A file compiled into the binary, with stored codings of it.
    ///
    /// What [`assets!`](crate::assets) emits where the directory held
    /// `app.js.br` or `app.js.gz` beside `app.js`. `encodings` is in the order
    /// the server prefers, which decides a tie between codings the client
    /// weighted equally. `path` and `etag` are as [`embedded`](Self::embedded)
    /// requires.
    ///
    /// # Panics
    ///
    /// Not here: mounting a set that holds this asset panics when `path` is not
    /// a legal path template, as [`embedded`](Self::embedded) says.
    /// [`try_embedded_with_codings`](Self::try_embedded_with_codings) refuses
    /// it where it is made.
    #[must_use]
    pub const fn embedded_with_codings(
        path: &'static str,
        bytes: &'static [u8],
        etag: &'static str,
        encodings: &'static [Encoded],
    ) -> Self {
        Self {
            path,
            bytes,
            etag,
            encodings,
        }
    }

    /// Every stored coding of this file, in the server's preference order.
    #[must_use]
    pub const fn encodings(&self) -> &'static [Encoded] {
        self.encodings
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
/// Mounted the way anything else is; a
/// [`Group`](crate::router::group::Group) supplies the prefix, the tag and the
/// interceptors.
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

/// What a set carrying no `Cache-Control` of its own sends: an hour, short
/// enough that unfingerprinted files are not stuck in caches.
pub(crate) const DEFAULT_CACHE_CONTROL: &str = "public, max-age=3600";

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
    ///
    /// `name` matches a whole file name, so `xindex.html` indexes nothing.
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
    /// A set that is *not* fingerprinted and claims to be is cached for a year
    /// by every client that saw it, with no way to take it back.
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
            // The index is a whole file name: `xindex.html` indexes nothing.
            if !(directory.is_empty() || directory.ends_with('/')) {
                return None;
            }
            // Keep the trailing slash a browser resolves relative links against.
            Some((asset, directory.to_owned()))
        })
    }
}

impl<C: Send + Sync + 'static> IntoEndpoints<C> for AssetSet {
    /// An asset carries no interceptors of its own.
    type Stacks = ();

    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        for asset in self.assets {
            sink.push(AssetEndpoint::new(
                *asset,
                asset.path,
                self.cache_control,
                &self.operation_id_prefix,
            ));
        }

        for (asset, directory) in self.indexed() {
            sink.push(AssetEndpoint::new(
                *asset,
                &directory,
                self.cache_control,
                &self.operation_id_prefix,
            ));
        }
    }
}

#[cfg(test)]
mod tests;
