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
//! Transcribed rather than cited, so a reviewer can check the reader against it
//! without leaving the file. Four sentences of section 14.1.1 and 14.1.2 do the
//! rest of the work:
//!
//! * *Byte ranges do not use the other-range specifier.* So anything that is
//!   neither an `int-range` nor a `suffix-range` is [`Ignored::Malformed`],
//!   even though `other-range` would admit it.
//! * *An int-range is invalid if the last-pos value is present and less than
//!   the first-pos*, and *a ranges-specifier is invalid if it contains any
//!   range-spec that is invalid* — so one bad spec invalidates the whole field
//!   rather than being dropped from it.
//! * *Recipients MUST anticipate potentially large decimal numerals and prevent
//!   parsing errors due to integer conversion overflows.* So a decimal numeral
//!   saturates at [`u64::MAX`] rather than failing to parse. Saturation is not
//!   a shortcut: a saturated `first-pos` is unsatisfiable, which is the 416; a
//!   saturated `last-pos` clamps to the end, which is what an out-of-range
//!   `last-pos` means anyway; and a saturated `suffix-length` exceeds the
//!   representation, which yields the whole of it.
//! * *A server that supports range requests MAY ignore or reject a Range header
//!   field that contains ... a set of many small ranges*, which section 17.15
//!   names as a denial-of-service indicator. [`MAX_RANGES`] is where Kynos
//!   draws that line.

use crate::http::{HeaderMap, Method, header};

/// The largest `range-set` Kynos reads.
///
/// A longer field is [`Ignored::TooManyRanges`] and answered with the whole
/// representation, which RFC 9110 section 14.2 permits outright. The number
/// reaches the description too, through [`pattern`], so the cap is a stated
/// fact rather than a surprise.
pub const MAX_RANGES: usize = 8;

/// The range unit Kynos understands, compared ASCII-case-insensitively.
///
/// Section 14.1: *all range unit names are case-insensitive*.
pub const UNIT: &str = "bytes";

/// The `pattern` an emitted `Range` parameter carries.
///
/// Built from [`MAX_RANGES`] rather than written out, so the cap the reader
/// enforces and the cap the description states are one number.
#[must_use]
pub fn pattern() -> String {
    format!(
        r"^bytes=(?:\d+-\d*|-\d+)(?:\s*,\s*(?:\d+-\d*|-\d+)){{0,{}}}$",
        MAX_RANGES - 1
    )
}

/// Why a `Range` field was not applied.
///
/// Every variant is a case RFC 9110 section 14.2 answers with *ignore it*, so
/// every one of them produces the whole representation and a 200. They are
/// named rather than collapsed into an [`Option`] because a reason nobody can
/// see is a reason nobody can test: [`crate::response::range`]'s suite counts
/// these variants against its cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// An `If-Range` field was sent.
    Conditional,
    /// The range unit is not `bytes`.
    ///
    /// Section 14.2: *an origin server MUST ignore a Range header field that
    /// contains a range unit it does not understand.*
    UnknownUnit,
    /// The field does not parse, or holds an invalid `range-spec`.
    Malformed,
    /// The `range-set` holds more than [`MAX_RANGES`] specs.
    TooManyRanges,
    /// The selected representation has zero length.
    ///
    /// Section 14.2: *a server that supports range requests MAY ignore a Range
    /// header field when the selected representation has no content*. Kynos
    /// takes that permission, because a zero-length part has no `incl-range`
    /// that could describe it.
    EmptyRepresentation,
}

/// One `range-spec`, as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Spec {
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
/// The method and the field are read together because two of the reasons to
/// ignore a `Range` are properties of the request rather than of the value.
pub(crate) fn read(method: &Method, headers: &HeaderMap) -> Result<Vec<Spec>, Ignored> {
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

    // Section 13.1.5 makes `If-Range` a precondition on applying the field, and
    // a `Ranged<T>` carries no validator for one to be evaluated against. See
    // the module documentation of `response::range` for why that is a narrow
    // position rather than a permanent one.
    if headers.contains_key(header::IF_RANGE) {
        return Err(Ignored::Conditional);
    }

    let value = value.to_str().map_err(|_| Ignored::Malformed)?;
    parse(value)
}

/// The `range-set` a `ranges-specifier` asks for.
pub(crate) fn parse(value: &str) -> Result<Vec<Spec>, Ignored> {
    let (unit, set) = value.trim().split_once('=').ok_or(Ignored::Malformed)?;
    if !unit.trim().eq_ignore_ascii_case(UNIT) {
        return Err(Ignored::UnknownUnit);
    }

    // Empty elements are skipped rather than refused: section 5.6.1.2 asks a
    // recipient to accept them in a `#` list, and the specification's own
    // example — `bytes= 0-999, 4500-5499, -1000` — carries whitespace the
    // strict ABNF for `range-set` does not admit either.
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

/// `1*DIGIT`, saturating at [`u64::MAX`] rather than overflowing.
///
/// ASCII digits only: a leading `+`, a Unicode digit or an empty run is not
/// `1*DIGIT` and produces `None`.
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
/// Section 14.1.2's three sentences, in order: an absent or over-long `last-pos`
/// is replaced with one less than the current length; an `int-range` whose
/// `first-pos` is not less than the current length is unsatisfiable; and a
/// `suffix-range` longer than the representation selects the whole of it, while
/// one of zero length is unsatisfiable.
///
/// `complete_length` is never zero here — a zero-length representation is
/// [`Ignored::EmptyRepresentation`] before resolution is reached.
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
fn resolve_one(spec: Spec, complete_length: u64) -> Option<(u64, u64)> {
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
