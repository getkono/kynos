//! Reading a `Range` field, and resolving what it asks for.
//!
//! # The grammar
//!
//! RFC 9110 section 14.1.1, restricted by section 14.1.2 to the two specifiers
//! the `bytes` unit defines:
//!
//! ```text
//! ranges-specifier = range-unit "=" range-set
//! range-set        = 1#range-spec
//! range-spec       = int-range
//!                  / suffix-range
//!                  / other-range
//!
//! int-range        = first-pos "-" [ last-pos ]
//! first-pos        = 1*DIGIT
//! last-pos         = 1*DIGIT
//!
//! suffix-range     = "-" suffix-length
//! suffix-length    = 1*DIGIT
//! ```
//!
//! * `other-range` is not used by `bytes`, so anything else is
//!   [`Ignored::Malformed`].
//! * One invalid `range-spec` (a `last-pos` below its `first-pos`) invalidates
//!   the whole field.
//! * A decimal numeral saturates at [`u64::MAX`] rather than failing to parse,
//!   as section 14.1.1 requires; every saturated value still resolves correctly.
//! * More than eight specs is [`Ignored::TooManyRanges`], the line Kynos draws
//!   against section 17.15's "many small ranges".
//!
//! # Sender grammar and recipient lenience
//!
//! The emitted `Range` parameter's `pattern` is the grammar a sender writes: a
//! case-insensitive unit, optional whitespace after the `=` and around each
//! comma, and one to eight specs. The parser reads more than that, as section
//! 5.6.1.2 asks of a recipient: it skips empty list elements, so
//! `bytes=0-1,,2-3` reads as two specs and an empty element does not count
//! toward the cap, and it ignores whitespace around the unit. A field only the
//! parser admits is still applied; the pattern does not tell a sender it may
//! write one.

use crate::http::{HeaderMap, HeaderValue, Method, etag, header};

/// The largest `range-set` Kynos reads.
///
/// A longer field is [`Ignored::TooManyRanges`] (RFC 9110 section 14.2). The
/// public docs spell the number out, so change them with it.
pub(crate) const MAX_RANGES: usize = 8;

/// The range unit Kynos understands, compared ASCII-case-insensitively.
///
/// Section 14.1: *all range unit names are case-insensitive*.
pub(crate) const UNIT: &str = "bytes";

/// The `pattern` an emitted `Range` parameter carries, built from [`UNIT`] and
/// [`MAX_RANGES`].
///
/// The sender grammar the module docs state: `OWS` after the `=` is section
/// 14.1.2's own example, and `OWS` around each comma is section 5.6.1.1's `1#`
/// list.
#[must_use]
pub(crate) fn pattern() -> String {
    const SPEC: &str = r"(?:\d+-\d*|-\d+)";
    const OWS: &str = r"[ \t]*";

    let unit: String = UNIT
        .chars()
        .flat_map(|letter| ['[', letter.to_ascii_uppercase(), letter, ']'])
        .collect();
    format!(
        "^{unit}={OWS}{SPEC}(?:{OWS},{OWS}{SPEC}){{0,{}}}$",
        MAX_RANGES - 1
    )
}

/// Why a `Range` field was not applied.
///
/// Every variant is a case RFC 9110 section 14.2 answers with *ignore it*, so
/// every one of them produces the whole representation and a 200.
///
/// `#[non_exhaustive]`: the set is Kynos's rather than the RFC's, so a further
/// reason to ignore a field may be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Ignored {
    /// No `Range` field was sent.
    Absent,
    /// More than one `Range` field was sent.
    ///
    /// `Range`'s value is a `ranges-specifier`, not a `#` list, so two fields
    /// do not join into one the way two `Accept` fields do.
    Repeated,
    /// The request method is not `GET`.
    ///
    /// Section 14.2: *a server MUST ignore a Range header field received with a
    /// request method that is unrecognized or for which range handling is not
    /// defined. For this specification, GET is the only method for which range
    /// handling is defined.*
    MethodUndefined,
    /// An `If-Range` condition was sent and does not hold.
    ///
    /// Section 13.1.5: *a recipient of an If-Range header field MUST ignore
    /// the Range header field if the If-Range condition evaluates to false.*
    /// A sender with no validator, such as every [`Ranged<T>`](super::Ranged)
    /// a handler builds, reaches this whenever `If-Range` is sent.
    Conditional,
    /// The range unit is not `bytes`.
    ///
    /// Section 14.2: *an origin server MUST ignore a Range header field that
    /// contains a range unit it does not understand.*
    UnknownUnit,
    /// The field does not parse, or holds an invalid `range-spec`.
    Malformed,
    /// The `range-set` holds more than eight specs.
    TooManyRanges,
    /// The selected representation has zero length.
    ///
    /// Section 14.2: *a server that supports range requests MAY ignore a Range
    /// header field when the selected representation has no content*; a
    /// zero-length part has no `incl-range` that could describe it.
    EmptyRepresentation,
}

/// One `range-spec`, as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Spec {
    /// `int-range`: an offset from the start, optionally to a last offset.
    Offsets {
        /// `first-pos`.
        first: u64,
        /// `last-pos`, absent when the range runs to the end.
        last: Option<u64>,
    },
    /// `suffix-range`: the last `length` bytes.
    Suffix {
        /// `suffix-length`.
        length: u64,
    },
}

/// The `range-set` a request asks for, or why the field is not applied.
///
/// `validator` is the selected representation's entity tag for `If-Range`, or
/// `None` where the sender has none.
pub(crate) fn read(
    method: &Method,
    headers: &HeaderMap,
    validator: Option<&str>,
) -> Result<Vec<Spec>, Ignored> {
    let mut sent = headers.get_all(header::RANGE).iter();
    let Some(value) = sent.next() else {
        return Err(Ignored::Absent);
    };

    if method != Method::GET {
        return Err(Ignored::MethodUndefined);
    }

    if sent.next().is_some() {
        return Err(Ignored::Repeated);
    }

    // Section 13.1.5 makes `If-Range` a precondition on applying the field.
    if let Some(condition) = headers.get(header::IF_RANGE) {
        if !holds(condition, validator) {
            return Err(Ignored::Conditional);
        }
    }

    let value = value.to_str().map_err(|_| Ignored::Malformed)?;
    parse(value)
}

/// Whether an `If-Range` condition holds against the representation's own
/// validator, per RFC 9110 section 13.1.5.
///
/// ```text
/// If-Range = entity-tag / HTTP-date
/// ```
///
/// One strong comparison settles every case: a weak tag on either side, an
/// `HTTP-date` (never equal to a quoted `opaque-tag`) and a missing validator
/// all answer `false`, which serves the whole representation.
fn holds(condition: &HeaderValue, validator: Option<&str>) -> bool {
    let (Ok(condition), Some(validator)) = (condition.to_str(), validator) else {
        return false;
    };

    etag::strong_match(condition.trim(), validator)
}

/// The `range-set` a `ranges-specifier` asks for.
pub(crate) fn parse(value: &str) -> Result<Vec<Spec>, Ignored> {
    let (unit, set) = value.trim().split_once('=').ok_or(Ignored::Malformed)?;
    if !unit.trim().eq_ignore_ascii_case(UNIT) {
        return Err(Ignored::UnknownUnit);
    }

    // Empty elements and optional whitespace are accepted, as section 5.6.1.2
    // asks of a `#` list.
    let written: Vec<&str> = set
        .split(',')
        .map(|element| element.trim_matches([' ', '\t']))
        .filter(|element| !element.is_empty())
        .collect();

    if written.is_empty() {
        return Err(Ignored::Malformed);
    }
    if written.len() > MAX_RANGES {
        return Err(Ignored::TooManyRanges);
    }

    written.into_iter().map(spec).collect()
}

/// One `range-spec`.
fn spec(written: &str) -> Result<Spec, Ignored> {
    if let Some(length) = written.strip_prefix('-') {
        return digits(length)
            .map(|length| Spec::Suffix { length })
            .ok_or(Ignored::Malformed);
    }

    let (first, last) = written.split_once('-').ok_or(Ignored::Malformed)?;
    let first = digits(first).ok_or(Ignored::Malformed)?;

    if last.is_empty() {
        return Ok(Spec::Offsets { first, last: None });
    }

    let last = digits(last).ok_or(Ignored::Malformed)?;
    if last < first {
        return Err(Ignored::Malformed);
    }

    Ok(Spec::Offsets {
        first,
        last: Some(last),
    })
}

/// `1*DIGIT` (ASCII only), saturating at [`u64::MAX`] rather than overflowing.
fn digits(written: &str) -> Option<u64> {
    if written.is_empty() || !written.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    Some(written.bytes().fold(0_u64, |value, byte| {
        value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(byte - b'0')))
            .unwrap_or(u64::MAX)
    }))
}

/// The byte offsets `specs` select from a representation of `complete_length`.
///
/// Unsatisfiable specs drop out, per RFC 9110 section 14.1.2. The request path
/// needs only the first, which it finds through [`resolve_one`] without
/// collecting the rest.
#[cfg(test)]
pub(crate) fn resolve(specs: &[Spec], complete_length: u64) -> Vec<(u64, u64)> {
    if complete_length == 0 {
        return Vec::new();
    }

    specs
        .iter()
        .filter_map(|spec| resolve_one(*spec, complete_length))
        .collect()
}

/// One spec, or `None` when it is unsatisfiable.
///
/// For a `complete_length` past zero: every caller has already answered an
/// empty representation whole, which section 14.2 permits.
pub(crate) fn resolve_one(spec: Spec, complete_length: u64) -> Option<(u64, u64)> {
    let end = complete_length.saturating_sub(1);

    match spec {
        Spec::Offsets { first, .. } if first >= complete_length => None,
        Spec::Offsets { first, last: None } => Some((first, end)),
        Spec::Offsets {
            first,
            last: Some(last),
        } => Some((first, last.min(end))),
        Spec::Suffix { length: 0 } => None,
        Spec::Suffix { length } => Some((complete_length.saturating_sub(length), end)),
    }
}
