use crate::{
    extract::{
        body::{binary::Binary, text::Text},
        media::Pdf,
    },
    response::negotiate::Accept,
};

#[test]
fn accept_prefers_quality_then_specificity() {
    let accepted = Accept::<(Text, Binary<Pdf>)>::parse("text/*;q=0.5, application/pdf;q=0.9")
        .expect("valid Accept header")
        .respond((Text(String::new()), Binary::default()))
        .expect("a representation matches");

    assert_eq!(accepted.selected, 1);
}

#[test]
fn accept_uses_the_most_specific_range_to_set_quality() {
    let accepted = Accept::<(Text, Binary<Pdf>)>::parse(
        "text/plain;q=0.1, text/*;q=0.9, application/pdf;q=0.5",
    )
    .expect("valid Accept header")
    .respond((Text(String::new()), Binary::default()))
    .expect("a representation matches");

    assert_eq!(accepted.selected, 1);
}

#[test]
fn accept_uses_first_offered_representation_to_break_ties() {
    let accepted = Accept::<(Text, Binary<Pdf>)>::parse("*/*")
        .expect("valid Accept header")
        .respond((Text(String::new()), Binary::default()))
        .expect("a representation matches");

    assert_eq!(accepted.selected, 0);
}

#[test]
fn accept_rejects_zero_quality_and_malformed_values() {
    assert!(
        Accept::<(Text, Binary<Pdf>)>::parse("text/plain;q=0")
            .expect("valid Accept header")
            .respond((Text(String::new()), Binary::default()))
            .is_err()
    );
    assert!(Accept::<(Text, Binary<Pdf>)>::parse("text/plain;q=1.1").is_err());
}
