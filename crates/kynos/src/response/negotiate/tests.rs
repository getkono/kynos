use crate::{
    extract::{
        body::{binary::Binary, text::Text},
        media::Pdf,
    },
    http::header,
    response::{IntoResponse, negotiate::Accept},
};

#[test]
fn accept_prefers_quality_then_specificity() {
    let accepted = Accept::<(Text, Binary<Pdf>)>::parse("text/*;q=0.5, application/pdf;q=0.9")
        .expect("valid Accept header")
        .choose::<(Text, Binary<Pdf>)>()
        .expect("a representation matches");

    assert_eq!(accepted, 1);
}

#[test]
fn accept_uses_the_most_specific_range_to_set_quality() {
    let accepted = Accept::<(Text, Binary<Pdf>)>::parse(
        "text/plain;q=0.1, text/*;q=0.9, application/pdf;q=0.5",
    )
    .expect("valid Accept header")
    .choose::<(Text, Binary<Pdf>)>()
    .expect("a representation matches");

    assert_eq!(accepted, 1);
}

#[test]
fn accept_uses_first_offered_representation_to_break_ties() {
    let accepted = Accept::<(Text, Binary<Pdf>)>::parse("*/*")
        .expect("valid Accept header")
        .choose::<(Text, Binary<Pdf>)>()
        .expect("a representation matches");

    assert_eq!(accepted, 0);
}

#[test]
fn accept_rejects_zero_quality_and_malformed_values() {
    assert!(
        Accept::<(Text, Binary<Pdf>)>::parse("text/plain;q=0")
            .expect("valid Accept header")
            .choose::<(Text, Binary<Pdf>)>()
            .is_err()
    );
    assert!(Accept::<(Text, Binary<Pdf>)>::parse("text/plain;q=1.1").is_err());
}

#[test]
fn a_negotiated_response_varies_on_accept_whichever_arm_wins() {
    for field in ["text/plain", "application/pdf", "*/*"] {
        let response = Accept::<(Text, Binary<Pdf>)>::parse(field)
            .expect("valid Accept header")
            .respond_with(
                &(),
                (
                    |(): &()| Text(String::new()),
                    |(): &()| Binary::<Pdf>::new(Vec::new()),
                ),
            )
            .expect("a representation matches")
            .into_response();

        let vary = response
            .headers()
            .get(header::VARY)
            .expect("a Vary on a response selected by Accept")
            .to_str()
            .expect("a visible-ASCII Vary");

        assert!(
            vary.split(',')
                .any(|name| name.trim().eq_ignore_ascii_case("accept")),
            "Accept: {field} selected a response whose Vary `{vary}` omits the Accept field"
        );
    }
}
