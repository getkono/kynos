//! The `Range` reader and section 14.1.2's satisfiability.
//!
//! The input is one `ranges-specifier`. The reader is held to a recogniser
//! transcribed from RFC 9110 section 14.1.1's grammar, which never consults it:
//! the two must read the same `range-spec`s, or ignore the field for the same
//! reason. The recogniser admits the grammar and exactly the lenient cases
//! listed on [`recognise`], so a leniency the reader grows or loses fails here.
//!
//! The field is then selected against representations of several lengths. An
//! ignored field selects the whole representation for that same reason at
//! every length; an understood one selects the first spec section 14.1.2 finds
//! satisfiable, cut to the representation, or refuses with a 416 naming the
//! length it was selected against.

#![no_main]

use kynos::{
    __private::fuzz,
    error::rejection::RangeRejection,
    response::range::{Range, Selection, spec::Ignored},
};
use libfuzzer_sys::fuzz_target;

const LENGTHS: [u64; 6] = [0, 1, 2, 10, 1_000, u64::MAX];

/// The most `range-spec`s Kynos reads, as its documentation states.
const MAX_RANGES: usize = 8;

/// One `range-spec`: `(Some(first-pos), last-pos)` for an `int-range`, and
/// `(None, Some(suffix-length))` for a `suffix-range`.
type Spec = (Option<u64>, Option<u64>);

fuzz_target!(|data: &[u8]| {
    let Ok(field) = std::str::from_utf8(data) else {
        return;
    };

    let expected = recognise(field);
    assert_eq!(fuzz::range_specs(field), expected, "{field:?}");

    let range = Range::<()>::parse(field);
    assert_eq!(
        range.ignored(),
        expected.as_ref().err().copied(),
        "{field:?}"
    );

    for length in LENGTHS {
        let selected = range.select(length);
        let specs = match &expected {
            Err(reason) => {
                assert_eq!(selected, Ok(Selection::Whole(*reason)));
                continue;
            }
            Ok(specs) => specs,
        };
        let wanted = if length == 0 {
            Ok(Selection::Whole(Ignored::EmptyRepresentation))
        } else {
            specs
                .iter()
                .find_map(|&spec| satisfied(spec, length))
                .map(|(first, last)| Selection::Part {
                    first,
                    last,
                    complete_length: length,
                })
                .ok_or(RangeRejection::NotSatisfiable {
                    complete_length: length,
                })
        };
        assert_eq!(selected, wanted, "{field:?} at {length}");
    }
});

/// The `range-spec`s `field` writes, transcribed from RFC 9110 section 14.1.1:
///
/// ```text
/// ranges-specifier = range-unit "=" range-set
/// range-set        = 1#range-spec
/// range-spec       = int-range / suffix-range / other-range
/// int-range        = first-pos "-" [ last-pos ]
/// suffix-range     = "-" suffix-length
/// ```
///
/// with `1#` expanded by section 5.6.1 to `range-spec *( OWS "," OWS
/// range-spec )`, and `bytes` defining no `other-range` (section 14.1.2).
///
/// The lenient cases, each one the reader admits beyond that grammar:
///
/// 1. Whitespace of any kind before and after the whole field, and around the
///    unit before `=`.
/// 2. `OWS` after `=` and at the end of the set, and empty list elements, as
///    section 5.6.1.2 asks a recipient to accept.
/// 3. The unit in any ASCII case (section 14.1).
/// 4. A numeral too large for 64 bits saturating at `u64::MAX` (section
///    14.1.1 asks that it not fail).
///
/// And the order the reader refuses in: a field with no `=` is malformed; then
/// a unit other than `bytes` is unknown, whatever follows; then a set of no
/// spec is malformed; then more than [`MAX_RANGES`] elements are too many,
/// before any is read; then any element that is not a `range-spec`, or an
/// `int-range` whose `last-pos` is below its `first-pos` (section 14.1.1's
/// *invalid*), makes the field malformed.
fn recognise(field: &str) -> Result<Vec<Spec>, Ignored> {
    let field = field.trim_matches(char::is_whitespace);
    let Some(equals) = field.find('=') else {
        return Err(Ignored::Malformed);
    };
    let unit = field[..equals].trim_matches(char::is_whitespace);
    if !unit.eq_ignore_ascii_case("bytes") {
        return Err(Ignored::UnknownUnit);
    }

    let mut elements = Vec::new();
    let mut element = String::new();
    for character in field[equals + 1..].chars().chain([',']) {
        if character == ',' {
            let written = element.trim_matches(|c| c == ' ' || c == '\t');
            if !written.is_empty() {
                elements.push(written.to_owned());
            }
            element.clear();
        } else {
            element.push(character);
        }
    }

    if elements.is_empty() {
        return Err(Ignored::Malformed);
    }
    if elements.len() > MAX_RANGES {
        return Err(Ignored::TooManyRanges);
    }
    elements
        .iter()
        .map(|element| range_spec(element).ok_or(Ignored::Malformed))
        .collect()
}

/// One `range-spec`, read character by character, or `None` where it is
/// not one or is invalid.
fn range_spec(element: &str) -> Option<Spec> {
    let mut characters = element.chars().peekable();
    let first = digits(&mut characters);
    if characters.next() != Some('-') {
        return None;
    }
    let last = digits(&mut characters);
    if characters.next().is_some() {
        return None;
    }

    match (first, last) {
        (None, None) => None,
        (None, Some(length)) => Some((None, Some(length))),
        (Some(first), Some(last)) if last < first => None,
        (Some(first), last) => Some((Some(first), last)),
    }
}

/// `1*DIGIT` at the front of `characters`, saturating at `u64::MAX`; `None`
/// where no digit is there.
fn digits(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<u64> {
    let mut value: Option<u128> = None;
    while let Some(digit) = characters.next_if(char::is_ascii_digit) {
        let digit = u128::from(digit) - u128::from('0');
        let so_far = value.unwrap_or(0);
        value = Some((so_far * 10 + digit).min(u128::from(u64::MAX)));
    }
    value.map(|value| u64::try_from(value).unwrap_or(u64::MAX))
}

/// The part `spec` selects from a representation of `length` octets, where
/// section 14.1.2 finds it satisfiable.
fn satisfied((first, last): Spec, length: u64) -> Option<(u64, u64)> {
    let end = length - 1;
    match (first, last) {
        (Some(first), _) if first >= length => None,
        (Some(first), None) => Some((first, end)),
        (Some(first), Some(last)) => Some((first, last.min(end))),
        (None, Some(0)) => None,
        (None, Some(suffix)) => Some((length - suffix.min(length), end)),
        (None, None) => unreachable!("no spec writes neither position"),
    }
}
