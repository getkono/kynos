//! Which `content` key declares an observed media type.
//!
//! Separate from [`super`], which checks a response against what that key
//! declares, because how a key matches a media type changes independently of
//! what a matched representation is held to.

use kynos_openapi::RefOr;

/// The representation declared for `media_type`, if one is.
///
/// A key may be a media range, and only the most specific matching key
/// applies: the exact key, so two keys differing only by parameter stay
/// distinct, then one ignoring the key's parameters, since `media_type`
/// arrives with its own already stripped, then `type/*`, then `*/*`. Keys
/// equally specific resolve to the first the map holds.
pub(super) fn declared_representation<'a>(
    content: &'a kynos_openapi::Map<RefOr<kynos_openapi::MediaType>>,
    media_type: &str,
) -> Option<(&'a str, &'a RefOr<kynos_openapi::MediaType>)> {
    content
        .iter()
        .enumerate()
        .filter_map(|(index, (name, entry))| {
            Some((
                specificity(name, media_type)?,
                index,
                (name.as_str(), entry),
            ))
        })
        .max_by_key(|&(rank, index, _)| (rank, std::cmp::Reverse(index)))
        .map(|(_, _, declared)| declared)
}

/// How specifically the `content` key `name` matches `media_type`, if at all.
fn specificity(name: &str, media_type: &str) -> Option<u8> {
    if name.eq_ignore_ascii_case(media_type) {
        return Some(3);
    }

    let (range, _) = name.split_once(';').unwrap_or((name, ""));
    let range = range.trim();
    if range.eq_ignore_ascii_case(media_type) {
        return Some(2);
    }

    // A range covers only a `type/subtype` value (RFC 9110 section 8.3.1).
    let (observed_type, observed_subtype) = media_type.split_once('/')?;
    if observed_type.is_empty() || observed_subtype.is_empty() {
        return None;
    }
    if range == "*/*" {
        return Some(0);
    }

    let (range_type, subtype) = range.split_once('/')?;
    (subtype == "*" && range_type.eq_ignore_ascii_case(observed_type)).then_some(1)
}

#[cfg(test)]
mod tests;
