//! Entity tags: reading a list of them, and the two ways to compare two.
//!
//! # The grammar
//!
//! RFC 9110 section 8.8.3:
//!
//! ```text
//! entity-tag = [ weak ] opaque-tag
//! weak       = %s"W/"
//! opaque-tag = DQUOTE *etagc DQUOTE
//! etagc      = %x21 / %x23-7E / obs-text
//!            ; VCHAR except double quotes, plus obs-text
//! ```
//!
//! Transcribed rather than cited, because one character decides the whole of
//! [`split`]: `etagc` admits `,` at `%x2C`. A comma *inside* the quotes is part
//! of the tag and only a comma outside them separates two, so the quotes are
//! the delimiter and the comma is not. A reader that splits on every comma
//! takes `"a,b"` for two tags and matches neither — a 200 where a 304 was owed,
//! and the kind of thing a comparator is either right about or silently wrong
//! about forever.
//!
//! # Why the comparison lives here
//!
//! Three call sites compare entity tags and no two of them share a feature:
//! [`middleware::conditional`](crate::middleware::conditional) is behind
//! `cache`, [`router::assets`](crate::router::assets) behind `assets`, and
//! `If-Range` — in [`response::range`](crate::response::range) — is behind
//! neither and wants the other comparison function besides. Nothing gated can
//! be the single implementation, so it sits with the field it reads instead,
//! beside [`cookie`](super::cookie): the other field whose grammar Kynos reads
//! rather than looks up.
//!
//! Public for the same reason `cookie` is. A handler that mints its own
//! validator and evaluates its own precondition needs exactly these four
//! functions, and the alternative to exporting them is every application
//! writing the comma scan again.

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderValue, header},
    schema::registry::Registry,
};

// `ETag` lives here rather than in `middleware::conditional` for the reason the
// comparison functions do, stated above: `response::range` is behind no feature
// and needs to mint one, and `conditional` is behind `cache`. A type the
// ungated caller cannot reach is not the single implementation.

/// An entity tag a handler attaches to its own response.
///
/// A [`HeaderParams`] group, so attaching one is *declaring* one and the
/// conflict check sees it. Return it through
/// [`WithHeaders`](crate::response::headers::WithHeaders).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ETag {
    /// The tag, without quotes or a weakness marker.
    pub value: String,
    /// Whether the tag is weak.
    pub weak: bool,
}

impl ETag {
    /// A strong tag: the representation is byte-for-byte this one.
    #[must_use]
    pub fn strong(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weak: false,
        }
    }

    /// A weak tag: the representation is equivalent, not identical.
    #[must_use]
    pub fn weak(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weak: true,
        }
    }

    /// The field value.
    #[must_use]
    pub fn encode(&self) -> Option<HeaderValue> {
        // RFC 9110 section 8.8.3: `etagc` is printable ASCII without `"`.
        if !self
            .value
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte) && byte != b'"')
        {
            return None;
        }

        let marker = if self.weak { "W/" } else { "" };
        HeaderValue::from_str(&format!("{marker}\"{}\"", self.value)).ok()
    }
}

impl HeaderParams for ETag {
    const NAMES: &'static [&'static str] = &["etag"];

    fn response_headers(
        registry: &mut Registry,
    ) -> kynos_openapi::Map<kynos_openapi::RefOr<kynos_openapi::Header>> {
        let _ = registry;

        let mut headers = kynos_openapi::Map::new();
        headers.insert(
            "ETag".to_owned(),
            kynos_openapi::RefOr::Item(
                kynos_openapi::Header::new(kynos_openapi::Schema::of_type(SchemaType::String))
                    .with_description("The entity tag of this representation"),
            ),
        );
        headers
    }
}

impl EncodeHeaders for ETag {
    fn encode(&self) -> Vec<(http::HeaderName, HeaderValue)> {
        Self::encode(self)
            .map(|value| vec![(header::ETAG, value)])
            .unwrap_or_default()
    }
}

/// `*`, which matches any current representation the server has.
pub const ANY: &str = "*";

/// The members of a `1#entity-tag` field value, trimmed.
///
/// Quote-aware, per the grammar above. An empty element is dropped rather than
/// refused: RFC 9110 section 5.6.1.2 asks a recipient of a `#` list to accept
/// them.
///
/// An iterator rather than a `Vec`, because every caller is deciding a
/// precondition on one request and a list nobody keeps is a list nobody should
/// have allocated. Each member borrows the field value, and the scan restarts
/// at each unquoted comma — which costs nothing, since an element boundary is
/// by construction a point at which no `opaque-tag` is open.
pub fn split(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);

    core::iter::from_fn(move || {
        loop {
            let remaining = rest?;

            let mut quoted = false;
            let separator = remaining.char_indices().find(|&(_, character)| {
                match character {
                    // `etagc` excludes DQUOTE and `opaque-tag` has no escape, so
                    // every quote is a delimiter and toggling on it is the whole
                    // of the scan.
                    '"' => {
                        quoted = !quoted;
                        false
                    }
                    ',' => !quoted,
                    _ => false,
                }
            });

            let candidate = if let Some((index, comma)) = separator {
                rest = Some(&remaining[index + comma.len_utf8()..]);
                &remaining[..index]
            } else {
                rest = None;
                remaining
            };

            let trimmed = candidate.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    })
}

/// Whether `tag` carries the weakness marker.
#[must_use]
pub fn is_weak(tag: &str) -> bool {
    tag.starts_with("W/")
}

/// A tag's `opaque-tag`, which is itself with any weakness marker removed.
#[must_use]
pub fn opaque(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

/// RFC 9110 section 8.8.3.2's *weak comparison*.
///
/// *Two entity tags are equivalent if their opaque-tags match
/// character-by-character, regardless of either or both being tagged as
/// "weak".* This is the one `If-None-Match` takes: a cache validation asks
/// whether the stored copy is still good enough, not whether it is identical.
#[must_use]
pub fn weak_match(left: &str, right: &str) -> bool {
    opaque(left) == opaque(right)
}

/// RFC 9110 section 8.8.3.2's *strong comparison*.
///
/// *Two entity tags are equivalent if both are not weak and their opaque-tags
/// match character-by-character.* This is the one `If-Range` takes, per section
/// 13.1.5 — a part is only safe to splice into a copy the client already holds
/// if the representation is byte-for-byte the one that copy came from. A weak
/// tag therefore satisfies nothing here, on either side.
#[must_use]
pub fn strong_match(left: &str, right: &str) -> bool {
    !is_weak(left) && !is_weak(right) && left == right
}

#[cfg(test)]
mod tests;
