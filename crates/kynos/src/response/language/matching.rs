//! Matching a language range against the tags a service offers.
//!
//! # Which scheme, and why not just one
//!
//! RFC 4647 section 3 defines several, and RFC 9110 section 12.5.4 declines to
//! pick: "Implementations can offer the most appropriate matching scheme for
//! their requirements." The two that matter here point in opposite directions.
//!
//! | Client asks | Service offers | Basic Filtering (3.3.1) | Lookup (3.4) |
//! | --- | --- | --- | --- |
//! | `en-GB` | `en` | no match | `en` |
//! | `en` | `en-GB` | `en-GB` | no match |
//!
//! Neither row is rare. RFC 9110 section 12.5.4's closing note is a complaint
//! about the first — "users might assume that on selecting 'en-gb', they will
//! be served any kind of English document if British English is not
//! available" — and the second is the ordinary shape of a catalogue keyed
//! `en-US`, `pt-BR`, `zh-Hans` answering clients that send bare `en`.
//!
//! So Kynos runs Lookup and falls back to Basic Filtering, ranking a Lookup
//! match above a filtering one. Wherever Lookup has an answer that answer wins,
//! which is what makes this an extension of section 3.4 rather than a third
//! scheme: the fallback only decides cases Lookup would have abandoned.
//!
//! A range and a tag that diverge mid-way — `en-GB` against `en-US` — match
//! under neither, and that is the honest answer rather than an oversight. They
//! are different representations, and a service that wants one served to the
//! other's clients offers `en` as well.

/// How a range and an offered tag relate.
///
/// Ordered deliberately: a greater value is a better relation, so ranking can
/// compare these directly. `Wildcard` is least because `*` says only "anything
/// will do", which cannot choose between two tags that both satisfy it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchKind {
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
/// `None` when they do not match under either scheme. Comparison is
/// case-insensitive, which section 3.3.1 requires and section 2.1.1 of RFC 5646
/// explains: case carries no meaning in a tag.
#[must_use]
pub fn classify(range: &str, tag: &str) -> Option<(MatchKind, usize)> {
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

    // They agreed for as long as the shorter of the two lasted, or they did
    // not match at all. `en-GB` against `en-US` stops here.
    if shared != range_length.min(tag_length) {
        return None;
    }

    match range_length.cmp(&tag_length) {
        std::cmp::Ordering::Equal => Some((MatchKind::Exact, shared)),

        // The tag is longer: `de-de` matches `de-DE-1996`. Section 3.3.1.
        std::cmp::Ordering::Less => Some((MatchKind::Extends, shared)),

        // The range is longer, so truncation may reach the tag -- but only at
        // the stops section 3.4 actually lands on. Its one subtlety is that a
        // singleton is "removed at the same time as [its] closest trailing
        // subtag", so `zh-Hant-CN-x-private1` falls straight to `zh-Hant-CN`
        // and `zh-Hant-CN-x` is never a stop.
        //
        // The walk is the way to say that, not a predicate on the tag. "A tag
        // ending in a singleton is not a stop" is the tempting shortcut and it
        // is wrong: `x-x-x` truncates to `x`, because the singleton removed on
        // the way there is the *middle* subtag rather than the trailing one.
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
