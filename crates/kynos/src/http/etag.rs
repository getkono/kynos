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
//! `etagc` admits `,`, so a list is split only on commas outside the quotes:
//! `"a,b"` is one tag.
//!
//! Only [`ETag`] is public. A handler that attaches one has the `Conditional`
//! interceptor or a ranged `Served` response evaluate the request's
//! preconditions against it.

use kynos_openapi::model::schema::types::SchemaType;

use crate::{
    extract::params::header::{EncodeHeaders, HeaderParams},
    http::{HeaderMap, HeaderValue, header},
    schema::registry::Registry,
};

// Ungated, beside the field it reads: its callers (`conditional` behind `cache`,
// `assets`, and ungated `response::range`) share no feature.

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
pub(crate) const ANY: &str = "*";

/// The members of a `1#entity-tag` field value, trimmed.
///
/// Quote-aware, per the module grammar. Empty elements are dropped (RFC 9110
/// section 5.6.1.2). Allocation-free.
pub(crate) fn split(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);

    core::iter::from_fn(move || {
        loop {
            let remaining = rest?;

            let mut quoted = false;
            let separator = remaining.char_indices().find(|&(_, character)| {
                match character {
                    // `opaque-tag` has no escape, so every quote toggles.
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

/// Whether `tag` is a strong `entity-tag` that [`matches`] can ever match.
///
/// RFC 9110 section 8.8.3 without `W/`, and without `obs-text`: preconditions
/// are read as visible ASCII, so such a tag could never match.
#[cfg(feature = "assets")]
#[must_use]
pub(crate) fn is_strong(tag: &str) -> bool {
    tag.len() >= 2
        && tag.starts_with('"')
        && tag.ends_with('"')
        && tag.as_bytes()[1..tag.len() - 1]
            .iter()
            .all(|&byte| byte == 0x21 || (0x23..=0x7e).contains(&byte))
}

/// Whether `tag` carries the weakness marker.
#[must_use]
pub(crate) fn is_weak(tag: &str) -> bool {
    tag.starts_with("W/")
}

/// A tag's `opaque-tag`, which is itself with any weakness marker removed.
#[must_use]
pub(crate) fn opaque(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

/// RFC 9110 section 8.8.3.2's *weak comparison*, which `If-None-Match` takes.
#[must_use]
pub(crate) fn weak_match(left: &str, right: &str) -> bool {
    opaque(left) == opaque(right)
}

/// RFC 9110 section 8.8.3.2's *strong comparison*, which `If-Range` (section
/// 13.1.5) and `If-Match` take. A weak tag on either side never matches.
#[must_use]
pub(crate) fn strong_match(left: &str, right: &str) -> bool {
    !is_weak(left) && !is_weak(right) && left == right
}

/// Whether an `If-None-Match` `field` names `current`, per RFC 9110 section
/// 13.1.2.
///
/// `*` matches anything; otherwise the [weak comparison](weak_match) applies.
/// A field that is not visible ASCII names nothing.
#[must_use]
pub(crate) fn matches(field: &HeaderValue, current: &str) -> bool {
    let Ok(text) = field.to_str() else {
        return false;
    };

    if text.trim() == ANY {
        return true;
    }

    split(text).any(|candidate| weak_match(candidate, current))
}

/// Whether an `If-Match` `field` holds for a representation tagged `current`,
/// per RFC 9110 section 13.1.1.
///
/// The [strong comparison](strong_match) applies. `*` holds even where
/// `current` is `None`: callers have already established a representation
/// exists. A field that is not visible ASCII names nothing.
#[must_use]
pub(crate) fn matches_strongly(field: &HeaderValue, current: Option<&str>) -> bool {
    let Ok(text) = field.to_str() else {
        return false;
    };

    if text.trim() == ANY {
        return true;
    }

    current.is_some_and(|current| split(text).any(|candidate| strong_match(candidate, current)))
}

/// Whether a request's `If-Match` holds for a representation tagged
/// `current`, or `None` where the request carries no `If-Match` at all.
///
/// Holds where [any line holds](matches_strongly). `None` tells an absent field
/// from a failed one, which section 13.1.4 needs. `current` is called only when
/// the field is present.
#[must_use]
pub(crate) fn if_match<S: AsRef<str>>(
    fields: &HeaderMap,
    current: impl FnOnce() -> Option<S>,
) -> Option<bool> {
    let mut lines = fields.get_all(header::IF_MATCH).iter().peekable();
    lines.peek()?;

    let current = current();
    let current = current.as_ref().map(AsRef::as_ref);
    Some(lines.any(|line| matches_strongly(line, current)))
}

#[cfg(test)]
mod tests;
