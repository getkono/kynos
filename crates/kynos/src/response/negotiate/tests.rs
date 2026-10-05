use crate::{
    error::rejection::NegotiationRejection,
    extract::{
        FromRequestParts,
        body::{binary::Binary, text::Text},
    },
    http::{HeaderValue, header, media::Pdf},
    response::{
        IntoResponse,
        negotiate::{Accept, representation::Representation},
    },
    schema::registry::Registry,
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

/// Every concatenation of at most `length` of `tokens`, the empty one
/// included.
fn every_field(tokens: &'static [&'static str], length: u32) -> impl Iterator<Item = String> {
    (0..=length).flat_map(move |length| {
        (0..tokens.len().pow(length)).map(move |mut index| {
            let mut field = String::new();
            for _ in 0..length {
                field.push_str(tokens[index % tokens.len()]);
                index /= tokens.len();
            }
            field
        })
    })
}

/// Every field over a closed alphabet of the grammar's own pieces parses or is
/// refused without panicking, and each outcome is the one its stage may give:
/// parsing refuses only as malformed, and choosing refuses only as not
/// acceptable.
///
/// A sweep rather than a property test: the alphabet is small enough to
/// close, and `proptest` is deliberately not a `kynos` dev-dependency.
#[test]
fn every_short_field_parses_or_is_refused_as_malformed() {
    const TOKENS: &[&str] = &["text", "/", "*", "plain", ";", "q=", "0.5", "1.1", ",", " "];

    for field in every_field(TOKENS, 5) {
        match Accept::<(Text, Binary<Pdf>)>::parse(&field) {
            Ok(accept) => match accept.choose::<(Text, Binary<Pdf>)>() {
                Ok(index) => assert!(index < 2, "{field:?} chose {index}"),
                Err(rejection) => assert!(
                    matches!(rejection, NegotiationRejection::NotAcceptable),
                    "{field:?} chose nothing as {rejection:?}"
                ),
            },
            Err(rejection) => assert!(
                matches!(rejection, NegotiationRejection::MalformedAccept { .. }),
                "{field:?} was refused as {rejection:?}"
            ),
        }
    }
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

/// Asserts `T` is offered under a media type its own 200 response describes.
///
/// The two halves of an offer: what negotiation matches `Accept` against, and
/// the `content` key the description lists. Disagreeing, a client that asked
/// for the documented type would be refused with a 406.
fn offered_under_what_it_describes<T: Representation>() {
    let described = T::responses(&mut Registry::default());
    let Some(kynos_openapi::RefOr::Item(ok)) = described.get(200) else {
        panic!("{} describes no inline 200 response", T::media_type());
    };

    assert!(
        ok.content.contains_key(T::media_type()),
        "offered under `{}` but described under {:?}",
        T::media_type(),
        ok.content.keys().collect::<Vec<_>>()
    );
}

#[test]
fn a_representation_is_offered_under_the_media_type_it_describes() {
    offered_under_what_it_describes::<Text>();
    offered_under_what_it_describes::<Binary<Pdf>>();
    #[cfg(feature = "json")]
    offered_under_what_it_describes::<crate::extract::body::json::Json<String>>();
    #[cfg(feature = "form")]
    offered_under_what_it_describes::<crate::extract::body::form::Form<String>>();
    #[cfg(feature = "multipart")]
    offered_under_what_it_describes::<crate::extract::body::multipart::MultipartForm<Fields>>();
}

/// A multipart form with no fields, which is all an offer's description needs.
#[cfg(feature = "multipart")]
struct Fields;

#[cfg(feature = "multipart")]
impl crate::schema::Schema for Fields {
    fn schema(_registry: &mut Registry) -> kynos_openapi::Schema {
        kynos_openapi::Schema::any()
    }
}

#[cfg(feature = "multipart")]
impl crate::response::codec::multipart::IntoMultipart for Fields {
    fn into_parts(self) -> Vec<crate::extract::body::multipart::Part> {
        Vec::new()
    }
}
