//! Matching a language range against the tags a service offers.
//!
//! RFC 4647 Lookup (3.4), falling back to Basic Filtering (3.3.1) where Lookup
//! has no answer; `docs/standards.md` records why. Ranges that diverge mid-way
//! (`en-GB` against `en-US`) match under neither.

/// How a range and an offered tag relate; a greater value is a better relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum MatchKind {
    /// The range was `*`, which RFC 4647 section 3.3.1 matches to any tag.
    Wildcard,
    /// The tag extends the range: RFC 4647 section 3.3.1 Basic Filtering.
    Extends,
    /// The range truncates to the tag: RFC 4647 section 3.4 Lookup.
    Truncates,
    /// The two are the same tag.
    Exact,
}

/// How `range` relates to `tag`, and how many subtags they share.
///
/// `None` when they match under neither scheme. Case-insensitive (RFC 4647
/// section 3.3.1, RFC 5646 section 2.1.1).
#[must_use]
pub(super) fn classify(range: &str, tag: &str) -> Option<(MatchKind, usize)> {
    if range == "*" {
        return Some((MatchKind::Wildcard, 0));
    }

    let range_subtags = range.split('-');
    let tag_subtags = tag.split('-');
    let range_length = range_subtags.clone().count();
    let tag_length = tag_subtags.clone().count();

    let shared = range_subtags
        .zip(tag_subtags)
        .take_while(|(from_range, from_tag)| from_range.eq_ignore_ascii_case(from_tag))
        .count();

    // They must agree for the whole of the shorter one.
    if shared != range_length.min(tag_length) {
        return None;
    }

    match range_length.cmp(&tag_length) {
        std::cmp::Ordering::Equal => Some((MatchKind::Exact, shared)),

        std::cmp::Ordering::Less => Some((MatchKind::Extends, shared)),

        // Walk section 3.4's truncation stops: a singleton goes together with
        // the subtag after it. A predicate on the tag's last subtag would be
        // wrong: `x-x-x` truncates to `x`.
        std::cmp::Ordering::Greater => {
            let subtags: Vec<&str> = range.split('-').collect();
            let mut length = subtags.len();

            while length > tag_length {
                let dropped = length - 1;
                length = if dropped > 0 && subtags[dropped - 1].len() == 1 {
                    dropped - 1
                } else {
                    dropped
                };
            }

            (length == tag_length).then_some((MatchKind::Truncates, shared))
        }
    }
}

/// Why a range in an `Accept-Language` field was dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RangeDefect {
    /// The entry held no range at all.
    Empty,
    /// The first subtag was not one to eight letters.
    PrimarySubtag,
    /// A later subtag was not one to eight letters or digits.
    Subtag,
    /// A `q` parameter was present and was not a qvalue.
    Weight,
}

/// One `language-range` from the field, with the weight it was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Preference {
    /// The range, folded to lowercase. `*` for the wildcard.
    range: String,
    /// The weight, in thousandths. Absent is 1, which section 12.4.2 defines.
    quality: u16,
    /// Position in the field, for tie-breaking.
    order: usize,
}

impl Preference {
    /// Reads one comma-separated entry.
    ///
    /// # Errors
    ///
    /// Returns why the entry is not a weighted range; callers drop it.
    pub(super) fn parse(entry: &str, order: usize) -> Result<Self, RangeDefect> {
        let mut segments = entry.trim().split(';');
        let range = segments.next().unwrap_or_default().trim();

        if range.is_empty() {
            return Err(RangeDefect::Empty);
        }

        if range != "*" {
            // `language-range = (1*8ALPHA *("-" 1*8alphanum)) / "*"` (RFC 4647
            // section 2.1), looser than a tag's grammar.
            let mut subtags = range.split('-');
            let primary = subtags.next().unwrap_or_default();
            if primary.is_empty()
                || primary.len() > 8
                || !primary.bytes().all(|byte| byte.is_ascii_alphabetic())
            {
                return Err(RangeDefect::PrimarySubtag);
            }
            for subtag in subtags {
                if subtag.is_empty()
                    || subtag.len() > 8
                    || !subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
                {
                    return Err(RangeDefect::Subtag);
                }
            }
        }

        let mut quality = 1_000;
        for parameter in segments {
            let Some((name, value)) = parameter.trim().split_once('=') else {
                return Err(RangeDefect::Weight);
            };
            if name.trim().eq_ignore_ascii_case("q") {
                quality = crate::http::quality::parse(value.trim()).ok_or(RangeDefect::Weight)?;
            }
        }

        Ok(Self {
            range: range.to_ascii_lowercase(),
            quality,
            order,
        })
    }
}

/// The offered tag a priority list selects, or `None` when it selects none.
///
/// Each offered tag takes the weight of the most specific range matching it,
/// so `*` only scores tags no other range named (RFC 9110 section 12.4.3).
/// Ties fall to client order, then match quality, then offer order; `q=0`
/// refuses a tag.
pub(super) fn select(preferences: &[Preference], offered: &[&str]) -> Option<usize> {
    offered
        .iter()
        .enumerate()
        .filter_map(|(index, tag)| score(preferences, tag).map(|score| (score, index)))
        .max_by(|(left, left_index), (right, right_index)| {
            left.cmp(right).then_with(|| right_index.cmp(left_index))
        })
        .map(|(_, index)| index)
}

/// What one offered tag is worth to this client.
///
/// Compared field by field. Client order ranks above match quality so that
/// `de, en` prefers `de` whatever order the service lists them in (RFC 4647
/// section 3.4).
type Score = (u16, std::cmp::Reverse<usize>, MatchKind, usize);

/// What `tag` is worth, through the most specific range that matches it.
///
/// Specificity is measured on the range (match kind, then subtag count), not
/// the shared prefix, which is equal for every range truncating to one tag.
fn score(preferences: &[Preference], tag: &str) -> Option<Score> {
    preferences
        .iter()
        .filter_map(|preference| {
            classify(&preference.range, tag).map(|(kind, depth)| (kind, depth, preference))
        })
        // The most specific range wins, so `fr;q=0.1, *;q=0.9` scores `fr` at
        // 0.1 (RFC 9110 section 12.4.3).
        .max_by_key(|(kind, _, preference)| {
            (
                *kind,
                preference.range.split('-').count(),
                std::cmp::Reverse(preference.order),
            )
        })
        .and_then(|(kind, depth, preference)| {
            (preference.quality != 0).then_some((
                preference.quality,
                std::cmp::Reverse(preference.order),
                kind,
                depth,
            ))
        })
}
