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
//! An embedded file is one `paths` key: a 200 with its media type, a 304, a
//! 206, a 416, and the `ETag`, `Cache-Control`, `Accept-Ranges` and
//! `Content-Range` each of those carries. There is no `Last-Modified` and no
//! `If-Modified-Since`, which is a decision rather than an omission — a strong
//! entity tag is the stronger validator, and sending a date obliges honouring a
//! request that carries one back. Sending neither half is consistent; sending
//! one is not.
//!
//! # A file is where a byte range has everything it needs
//!
//! RFC 9110 section 14.1.2 defines a byte range over octets of a known length,
//! and both modes have exactly that. So both serve one, through the reader and
//! the satisfiability rule in
//! [`response::range`](crate::response::range) rather than through anything of
//! their own — `router::assets::range` holds what the two modes share and
//! where they part.

pub mod media;

use std::borrow::Cow;

use crate::router::assets::endpoint::AssetEndpoint;
use crate::router::endpoint::set::{Endpoints, IntoEndpoints};

pub mod endpoint;

mod range;

#[cfg(feature = "assets-fs")]
pub mod fs;

/// One file an asset set serves.
///
/// `const`-constructible throughout, so an embedded set is one `static` and
/// costs nothing to hold.
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
/// nothing here, which is what makes the tag trustworthy — these octets exist
/// on disk at compile time and are hashed like any other file.
///
/// # Why each coding carries its own tag
///
/// RFC 9110 section 8.8.1: a strong entity tag names one representation, and
/// "different representations of the same resource" must not share one. Section
/// 14.1.2 then calculates a byte range against "the encoded sequence of bytes"
/// when a content coding is applied. One tag over both forms makes section
/// 13.1.5's `If-Range` succeed on a resume it exists to refuse, and the client
/// splices encoded octets onto an identity prefix — nothing errors and the file
/// is wrong.
///
/// A tag per stored coding is what makes both properties hold at once, which
/// [`Compression`](crate::middleware::compression) cannot do: it is handed a
/// response whose tag and range were already decided.
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
    /// `Content-Encoding` carries; `etag` is quoted and minted from `bytes`
    /// rather than from the file it encodes.
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
    /// `/`-separated with no leading slash; `etag` is quoted and ready for the
    /// field.
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
    /// weighted equally.
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
