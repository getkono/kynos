//! Refusing a hand-built asset where it is made.
//!
//! [`assets!`](crate::assets) only ever emits what these checks accept, so
//! they exist for an [`Asset`] built by hand — what
//! [`Asset::embedded`] would otherwise leave to a panic at mount, or to the
//! wire.

use kynos_openapi::PathTemplate;

use crate::router::assets::{Asset, Encoded};

/// Why [`Asset::try_embedded`] refused a hand-built asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidAsset {
    /// The path is not one an asset can be served and described at.
    #[error("`{path}` is not a relative path made of literal segments")]
    Path {
        /// The path as given.
        path: &'static str,
    },
    /// An entity tag is not strong, quoted and visible ASCII.
    #[error("`{etag}` is not a strong, quoted entity tag")]
    ETag {
        /// The tag as given.
        etag: &'static str,
    },
    /// A stored coding is not one `Content-Encoding` can name.
    #[error("`{coding}` is not a content coding a stored form can carry")]
    Coding {
        /// The coding as given.
        coding: &'static str,
    },
}

impl Asset {
    /// A file compiled into the binary, checked before it can be mounted.
    ///
    /// For an `Asset` built by hand rather than by [`assets!`](crate::assets):
    /// what [`embedded`](Self::embedded) leaves to a panic at mount, or to the
    /// wire, is refused here instead. Not `const`, because the path is checked
    /// by the same parser mounting uses, so an asset this accepts mounts at
    /// its own path. Hold a hand-built set in a `LazyLock` to get the
    /// `&'static [Asset]` that
    /// [`AssetSet::embedded`](crate::router::assets::AssetSet::embedded) takes.
    ///
    /// ```
    /// use kynos::router::assets::{Asset, error::InvalidAsset};
    ///
    /// let asset = Asset::try_embedded("docs/app.css", b"body{}", "\"v1\"")?;
    /// assert_eq!(asset.path(), "docs/app.css");
    ///
    /// assert_eq!(
    ///     Asset::try_embedded("docs/app.css", b"body{}", "v1"),
    ///     Err(InvalidAsset::ETag { etag: "v1" }),
    /// );
    /// # Ok::<(), InvalidAsset>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`InvalidAsset::Path`] unless `path` is relative, `/`-separated, and
    /// holds only segments a client can send and a path template carries
    /// literally: none empty, none `.` or `..` whether written out or as
    /// `%2e` (a client removes those before sending, RFC 3986 section 5.2.4),
    /// and no `{`, which would make it a
    /// variable matching paths that are not this file.
    ///
    /// [`InvalidAsset::ETag`] unless `etag` is a strong, quoted `entity-tag`
    /// (RFC 9110 section 8.8.3): a byte range is resumed against it, and only
    /// a strong validator may be (section 13.1.5). The grammar's `obs-text`
    /// is refused too, because a precondition field is compared as visible
    /// ASCII and a tag holding anything else would never match.
    pub fn try_embedded(
        path: &'static str,
        bytes: &'static [u8],
        etag: &'static str,
    ) -> Result<Self, InvalidAsset> {
        Self::try_embedded_with_codings(path, bytes, etag, &[])
    }

    /// [`embedded_with_codings`](Self::embedded_with_codings), checked as
    /// [`try_embedded`](Self::try_embedded) checks.
    ///
    /// # Errors
    ///
    /// What [`try_embedded`](Self::try_embedded) refuses, and for each stored
    /// coding: [`InvalidAsset::ETag`] for a tag it would refuse, and
    /// [`InvalidAsset::Coding`] for a coding that is not an RFC 9110 `token`
    /// (section 5.6.2) — `Content-Encoding` could not carry it, and the encoded
    /// octets would be served unlabelled — or that is `identity`, which section
    /// 8.4.1 says does not appear there.
    pub fn try_embedded_with_codings(
        path: &'static str,
        bytes: &'static [u8],
        etag: &'static str,
        encodings: &'static [Encoded],
    ) -> Result<Self, InvalidAsset> {
        if !is_asset_path(path) {
            return Err(InvalidAsset::Path { path });
        }
        if !crate::http::etag::is_strong(etag) {
            return Err(InvalidAsset::ETag { etag });
        }
        for encoded in encodings {
            if !crate::http::is_token(encoded.coding)
                || encoded.coding.eq_ignore_ascii_case("identity")
            {
                return Err(InvalidAsset::Coding {
                    coding: encoded.coding,
                });
            }
            if !crate::http::etag::is_strong(encoded.etag) {
                return Err(InvalidAsset::ETag { etag: encoded.etag });
            }
        }

        Ok(Self::embedded_with_codings(path, bytes, etag, encodings))
    }
}

/// Relative, `/`-separated, no empty or dot segment, and a template with no
/// variable.
fn is_asset_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['{', '}'])
        && !path.split('/').any(is_dot_segment)
        && PathTemplate::parse(format!("/{path}")).is_ok()
}

/// `.` or `..`, in either spelling: a client removes `%2e` dot segments as it
/// removes literal ones, so either kind names a path no request arrives at.
fn is_dot_segment(segment: &str) -> bool {
    let mut rest = segment;
    let mut dots = 0;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('.') {
            rest = after;
        } else if rest
            .as_bytes()
            .get(..3)
            .is_some_and(|head| head.eq_ignore_ascii_case(b"%2e"))
        {
            rest = &rest[3..];
        } else {
            return false;
        }
        dots += 1;
    }
    matches!(dots, 1 | 2)
}
