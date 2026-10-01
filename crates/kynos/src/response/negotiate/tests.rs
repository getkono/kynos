use crate::{
    error::rejection::NegotiationRejection,
    extract::{
        FromRequestParts,
        body::{binary::Binary, text::Text},
        media::Pdf,
    },
    http::{HeaderValue, header},
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

/// Whether `value` is refused as a malformed field rather than parsed.
///
/// The variant is the point: a malformed field is a 400, and a field that
/// parsed but matched nothing is a 406.
fn malformed(value: &str) -> bool {
    matches!(
        Accept::<(Text, Binary<Pdf>)>::parse(value),
        Err(NegotiationRejection::MalformedAccept { .. })
    )
}

/// A media range with no `/` is not a media range.
#[test]
fn a_range_without_a_slash_is_malformed() {
    for value in ["", "text", "text/plain, html"] {
        assert!(malformed(value), "{value:?} was not refused as malformed");
    }
}

/// Each half of a range must be present, and `*` may stand for a subtype only
/// under a `*` type (RFC 9110 section 12.5.1).
#[test]
fn a_range_missing_a_half_or_wildcarding_only_its_type_is_malformed() {
    for value in ["/plain", "text/", "*/plain"] {
        assert!(malformed(value), "{value:?} was not refused as malformed");
    }
    // The controls: a wildcard subtype, and both halves wildcarded.
    assert!(Accept::<(Text, Binary<Pdf>)>::parse("text/*").is_ok());
    assert!(Accept::<(Text, Binary<Pdf>)>::parse("*/*").is_ok());
}

/// A parameter is `name=value`; a bare name is not one.
#[test]
fn a_parameter_without_a_value_is_malformed() {
    assert!(malformed("text/plain;level"));
    // The control: a parameter other than `q` is read and ignored.
    assert!(Accept::<(Text, Binary<Pdf>)>::parse("text/plain;level=1").is_ok());
}

/// A `q` the qvalue grammar cannot express refuses the field, whatever case
/// its name is written in.
#[test]
fn a_quality_that_is_not_a_qvalue_is_malformed() {
    for value in [
        "text/plain;q=1.1",
        "text/plain;q=x",
        "text/plain;Q=",
        "*/*; q = -1",
    ] {
        assert!(malformed(value), "{value:?} was not refused as malformed");
    }
}

/// A field whose octets are not visible ASCII is refused before it is parsed.
#[tokio::test]
async fn a_field_that_is_not_text_is_malformed() {
    let mut parts = http::Request::new(()).into_parts().0;
    parts
        .headers
        .append(header::ACCEPT, HeaderValue::from_static("text/plain"));
    parts.headers.append(
        header::ACCEPT,
        HeaderValue::from_bytes(b"text/\xff").expect("an opaque field value"),
    );

    let extracted = Accept::<(Text, Binary<Pdf>)>::from_request_parts(&mut parts, &()).await;

    assert!(
        matches!(extracted, Err(NegotiationRejection::MalformedAccept { .. })),
        "{extracted:?}"
    );
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
