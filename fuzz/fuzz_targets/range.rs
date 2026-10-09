//! The `Range` reader and section 14.1.2's satisfiability.
//!
//! The input is one `ranges-specifier`, selected against representations of
//! several lengths. A field read without the request around it can only be
//! ignored for its unit, its syntax or its length, and an ignored field selects
//! the whole representation for that same reason at every length. An
//! understood one selects a part lying inside the representation, or refuses
//! with a 416 naming the length it was selected against.

#![no_main]

use kynos::{
    error::rejection::RangeRejection,
    response::range::{Range, Selection, spec::Ignored},
};
use libfuzzer_sys::fuzz_target;

const LENGTHS: [u64; 6] = [0, 1, 2, 10, 1_000, u64::MAX];

fuzz_target!(|data: &[u8]| {
    let Ok(field) = std::str::from_utf8(data) else {
        return;
    };

    let range = Range::<()>::parse(field);
    let ignored = range.ignored();
    assert!(
        matches!(
            ignored,
            None | Some(Ignored::UnknownUnit | Ignored::Malformed | Ignored::TooManyRanges)
        ),
        "{field:?} ignored as {ignored:?}"
    );

    for length in LENGTHS {
        let selected = range.select(length);
        match (ignored, selected) {
            (Some(reason), selected) => assert_eq!(selected, Ok(Selection::Whole(reason))),
            (None, Ok(Selection::Whole(reason))) => {
                assert_eq!((length, reason), (0, Ignored::EmptyRepresentation));
            }
            (
                None,
                Ok(Selection::Part {
                    first,
                    last,
                    complete_length,
                }),
            ) => {
                assert_eq!(complete_length, length);
                assert!(
                    first <= last && last < length,
                    "{field:?} at {length}: {first}-{last}"
                );
            }
            (None, Err(RangeRejection::NotSatisfiable { complete_length })) => {
                assert_eq!(complete_length, length);
            }
            (None, Err(other)) => panic!("{field:?} at {length}: {other:?}"),
        }
    }
});
