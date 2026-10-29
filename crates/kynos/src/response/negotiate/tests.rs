use crate::{
    error::rejection::NegotiationRejection,
    extract::{
        FromRequestParts,
        body::{binary::Binary, text::Text},
    },
    http::{
        HeaderValue, header,
        media::{Html, Pdf},
    },
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

/// An offer's parameters are not part of its subtype (RFC 9110 section
/// 12.5.1), so `text/html; charset=utf-8` matches a `text/html` range rather
/// than only `*/*`.
#[test]
fn an_offer_carrying_parameters_matches_its_type_and_subtype() {
    for value in [
        "text/html",
        "TEXT/HTML",
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    ] {
        let accepted = Accept::<(Text, Binary<Html>)>::parse(value)
            .expect("valid Accept header")
            .choose::<(Text, Binary<Html>)>()
            .ok();

        assert_eq!(accepted, Some(1), "{value:?} did not choose the HTML offer");
    }
}

/// RFC 9110 section 5.6.6: a parameter value may be a `quoted-string`, so a
/// `,` or `;` inside one separates neither list members nor parameters.
#[test]
fn a_quoted_parameter_value_may_hold_a_comma_or_a_semicolon() {
    for value in [
        "application/pdf;profile=\"a,b\";q=0.9, text/plain;q=0.1",
        "application/pdf;profile=\"a;q=0\";q=0.9, text/plain;q=0.1",
        "application/pdf;profile=\"a\\\",b\";q=0.9, text/plain;q=0.1",
    ] {
        let accepted = Accept::<(Text, Binary<Pdf>)>::parse(value)
            .unwrap_or_else(|rejection| panic!("{value:?} was refused: {rejection:?}"))
            .choose::<(Text, Binary<Pdf>)>()
            .ok();

        assert_eq!(accepted, Some(1), "{value:?} did not choose the PDF offer");
    }
}

/// At equal quality the offer a more specific range matched wins, whichever
/// was offered first, and between ranges equally specific the one the client
/// listed first wins.
#[test]
fn at_equal_quality_specificity_then_the_clients_order_decides() {
    let chosen = |field: &str| {
        Accept::<(Text, Binary<Pdf>)>::parse(field)
            .expect("valid Accept header")
            .choose::<(Text, Binary<Pdf>)>()
            .expect("a representation matches")
    };

    // `text/plain` is offered first and matches only `text/*`.
    assert_eq!(chosen("text/*, application/pdf"), 1);
    assert_eq!(chosen("application/pdf, text/*"), 1);
    assert_eq!(chosen("*/*, application/pdf"), 1);
    // Both exact: the client's order, not the offer's.
    assert_eq!(chosen("application/pdf, text/plain"), 1);
    assert_eq!(chosen("text/plain, application/pdf"), 0);
    // Both behind the same wildcard: the offer's order is all that is left.
    assert_eq!(chosen("*/*"), 0);
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

/// A recipient ignores empty list elements (RFC 9110 section 5.6.1.2), which
/// is also what a comma join of repeated field lines leaves behind.
#[test]
fn an_empty_list_element_is_ignored() {
    for value in [
        "application/pdf,",
        "text/plain;q=0.5, , application/pdf",
        ", application/pdf",
        "application/pdf, \t,",
    ] {
        let accepted = Accept::<(Text, Binary<Pdf>)>::parse(value)
            .unwrap_or_else(|rejection| panic!("{value:?} was refused as {rejection:?}"))
            .choose::<(Text, Binary<Pdf>)>()
            .ok();

        assert_eq!(accepted, Some(1), "{value:?} did not choose the PDF offer");
    }
}

/// `Accept` is a `#` list, which may hold no element at all: a field of only
/// empty elements is well-formed, accepts nothing, and so is a 406 rather than
/// a 400.
#[test]
fn a_field_of_only_empty_elements_accepts_nothing() {
    for value in ["", " ", ",", " , ,\t"] {
        let chosen = Accept::<(Text, Binary<Pdf>)>::parse(value)
            .unwrap_or_else(|rejection| panic!("{value:?} was refused as {rejection:?}"))
            .choose::<(Text, Binary<Pdf>)>();

        assert!(
            matches!(chosen, Err(NegotiationRejection::NotAcceptable)),
            "{value:?} chose {chosen:?}"
        );
    }
}

/// A repeated field line that is empty adds an empty element to the joined
/// list, not a malformed one.
#[tokio::test]
async fn an_empty_repeated_field_line_is_ignored() {
    let mut parts = http::Request::new(()).into_parts().0;
    parts
        .headers
        .append(header::ACCEPT, HeaderValue::from_static("application/pdf"));
    parts
        .headers
        .append(header::ACCEPT, HeaderValue::from_static(""));

    let chosen = Accept::<(Text, Binary<Pdf>)>::from_request_parts(&mut parts, &())
        .await
        .expect("a well-formed Accept list")
        .choose::<(Text, Binary<Pdf>)>()
        .ok();

    assert_eq!(chosen, Some(1));
}

/// A media range with no `/` is not a media range.
#[test]
fn a_range_without_a_slash_is_malformed() {
    for value in ["text", "text/plain, html", ";q=1", "application/pdf, ;q=1"] {
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
    offered_under_what_it_describes::<
        crate::extract::body::text_as::TextAs<crate::http::media::Html>,
    >();
    offered_under_what_it_describes::<
        crate::extract::body::codec::Encoded<a_body_in_an_outside_format::Tally, u64>,
    >();
    #[cfg(feature = "json")]
    offered_under_what_it_describes::<crate::extract::body::json::Json<String>>();
    #[cfg(feature = "json")]
    offered_under_what_it_describes::<
        crate::extract::body::json_as::JsonAs<crate::http::media::Json, String>,
    >();
    #[cfg(feature = "form")]
    offered_under_what_it_describes::<crate::extract::body::form::Form<String>>();
    #[cfg(feature = "multipart")]
    offered_under_what_it_describes::<crate::extract::body::multipart::MultipartForm<Fields>>();
    // `()` is the empty message, which `prost` implements `Message` for.
    #[cfg(feature = "protobuf")]
    offered_under_what_it_describes::<crate::extract::body::protobuf::Protobuf<()>>();
}

/// The handler [`negotiation_end_to_end`] mounts.
async fn report(
    accept: Accept<(Text, Binary<Pdf>)>,
) -> Result<crate::response::negotiate::Negotiated<(Text, Binary<Pdf>)>, NegotiationRejection> {
    accept.respond_with(
        &(),
        (
            |(): &()| Text("report".to_owned()),
            |(): &()| Binary::<Pdf>::new(b"%PDF".to_vec()),
        ),
    )
}

/// Negotiation through a built router: a request with no `Accept` is served
/// as `*/*` would be, the offer's first alternative; one asking for the second
/// gets it; one asking for neither is a live 406 problem. And the description
/// states the alternatives as one 200 carrying both media types, beside the
/// 400 and 406 the extractor declares.
#[tokio::test]
async fn negotiation_end_to_end() {
    use http_body_util::BodyExt as _;

    let router =
        crate::Router::<()>::new().mount(crate::router::endpoint::builder::EndpointBuilder::<
            (),
            _,
            _,
        >::new(
            kynos_openapi::Method::Get,
            kynos_openapi::PathTemplate::parse("/report").expect("a usable path template"),
            report,
        ));

    let document =
        serde_json::to_value(router.openapi().expect("a describable router")).expect("JSON");
    let responses = &document["paths"]["/report"]["get"]["responses"];
    let statuses: Vec<&String> = responses.as_object().expect("a map").keys().collect();
    assert_eq!(statuses, ["200", "400", "406"]);
    let offered: Vec<&String> = responses["200"]["content"]
        .as_object()
        .expect("a content map")
        .keys()
        .collect();
    // Keyed, so read in key order rather than the offer's.
    assert_eq!(offered, ["application/pdf", "text/plain"]);

    let service = router.build(()).expect("a describable router");
    let call = |accept: Option<&'static str>| {
        let mut request = ::http::Request::builder().uri("/report");
        if let Some(accept) = accept {
            request = request.header(header::ACCEPT, accept);
        }
        let request = request
            .body(crate::http::body::Body::empty())
            .expect("a well-formed request");
        let service = &service;
        async move {
            let response = service.call(request).await;
            let status = response.status();
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .map(|value| value.to_str().expect("a printable field").to_owned());
            let body = response
                .into_body()
                .collect()
                .await
                .expect("a readable body")
                .to_bytes();
            (status, content_type, body)
        }
    };

    let (status, content_type, body) = call(None).await;
    assert_eq!(status, ::http::StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(body.as_ref(), b"report");

    let (status, content_type, body) = call(Some("application/pdf")).await;
    assert_eq!(status, ::http::StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/pdf"));
    assert_eq!(body.as_ref(), b"%PDF");

    let (status, content_type, body) = call(Some("image/png")).await;
    assert_eq!(status, ::http::StatusCode::NOT_ACCEPTABLE);
    assert_eq!(content_type.as_deref(), Some("application/problem+json"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("a problem document")["status"],
        406
    );
}

/// Two offers are one media type to `Accept` when their type and subtype
/// match ignoring case, whatever parameters either carries. `Accept` trims
/// each half on its own, so whitespace beside the slash separates nothing.
#[test]
fn offers_accept_cannot_tell_apart_share_a_media_type() {
    use super::representation::same_media_type;

    for (left, right) in [
        ("application/json", "application/json"),
        ("text/plain", "TEXT/Plain"),
        ("text/plain", "text/plain; charset=iso-8859-1"),
        ("text/html; charset=utf-8", " text/html ;level=1"),
        ("text/plain", "text/ plain"),
        ("text /plain", "text/plain"),
    ] {
        assert!(same_media_type(left, right), "{left:?} and {right:?}");
    }
    // The controls: another subtype, a structured suffix, and another type.
    for (left, right) in [
        ("text/plain", "text/html; charset=utf-8"),
        ("application/json", "application/problem+json"),
        ("text/plain", "application/plain"),
    ] {
        assert!(!same_media_type(left, right), "{left:?} and {right:?}");
    }
}

/// Every pair is compared, not only neighbours.
#[test]
#[should_panic(expected = "`Negotiated` offers two alternatives under one media type")]
fn a_repeated_media_type_apart_from_its_twin_is_refused() {
    super::representation::distinct(&["text/plain", "application/pdf", "Text/Plain"]);
}

#[test]
fn distinct_media_types_are_admitted() {
    super::representation::distinct(&["text/plain", "application/pdf", "text/html"]);
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

/// A JSON document under a declared media type: written under that type
/// verbatim, described under it, and chosen by an `Accept` naming it.
#[cfg(feature = "json")]
mod a_json_body_under_a_declared_media_type {
    use http_body_util::BodyExt as _;

    use crate::{
        extract::body::{json::Json, json_as::JsonAs},
        http::{StatusCode, header, media::MediaType},
        response::{IntoResponse, Responses, negotiate::Accept},
        schema::registry::Registry,
    };

    struct V1;

    impl MediaType for V1 {
        const MEDIA_TYPE: &'static str = "application/vnd.acme.v1+json";
    }

    struct V2;

    impl MediaType for V2 {
        const MEDIA_TYPE: &'static str = "application/vnd.acme.v2+json; charset=utf-8";
    }

    #[tokio::test]
    async fn is_written_as_json_under_its_media_type() {
        let response = JsonAs::<V2, Vec<u32>>::new(vec![1, 2]).into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .map(crate::http::HeaderValue::as_bytes),
            Some(V2::MEDIA_TYPE.as_bytes())
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("an in-memory body")
            .to_bytes();
        assert_eq!(
            serde_json::from_slice::<Vec<u32>>(&body).expect("JSON"),
            [1, 2]
        );
    }

    #[test]
    fn is_described_under_its_media_type_over_the_schema_of_its_document() {
        let described = JsonAs::<V1, u32>::responses(&mut Registry::default());
        let Some(kynos_openapi::RefOr::Item(ok)) = described.get(200) else {
            panic!("no inline 200 response");
        };

        assert_eq!(ok.content.keys().collect::<Vec<_>>(), [V1::MEDIA_TYPE]);
        assert_eq!(
            ok.content[V1::MEDIA_TYPE].as_item().expect("inline").schema,
            Some(Registry::default().resolve::<u32>())
        );
    }

    #[test]
    fn an_accept_naming_it_chooses_it() {
        type Offer = (Json<u32>, JsonAs<V1, u32>, JsonAs<V2, u32>);

        for (field, chosen) in [
            ("application/json", 0),
            ("application/vnd.acme.v1+json", 1),
            ("Application/VND.Acme.V2+JSON", 2),
            ("application/vnd.acme.v2+json, application/json;q=0.5", 2),
        ] {
            let accepted = Accept::<Offer>::parse(field)
                .expect("valid Accept header")
                .choose::<Offer>()
                .ok();
            assert_eq!(accepted, Some(chosen), "Accept: {field}");
        }
    }
}

/// A text body under a declared media type is chosen by an `Accept` naming
/// that type, beside `Text`, whose `text/plain` it is told apart from.
#[test]
fn an_accept_naming_a_declared_text_type_chooses_it() {
    use crate::{extract::body::text_as::TextAs, http::media::MediaType};

    struct Csv;

    impl MediaType for Csv {
        const MEDIA_TYPE: &'static str = "text/csv";
    }

    type Offer = (Text, TextAs<Csv>);

    for (field, chosen) in [
        ("text/plain", 0),
        ("text/csv", 1),
        ("TEXT/CSV; charset=utf-8", 1),
        ("text/csv, text/plain;q=0.5", 1),
        ("text/*", 0),
    ] {
        let accepted = Accept::<Offer>::parse(field)
            .expect("valid Accept header")
            .choose::<Offer>()
            .ok();
        assert_eq!(accepted, Some(chosen), "Accept: {field}");
    }
}

/// A body in an application's own format is described under its codec's media
/// type verbatim and chosen by an `Accept` naming it; what it writes is
/// `codec/tests.rs`'s.
mod a_body_in_an_outside_format {
    use crate::{
        extract::body::{
            codec::{BodyCodec, DecodeError, Encoded},
            text::Text,
        },
        response::{Responses, negotiate::Accept},
        schema::registry::Registry,
    };

    /// A decimal tally, carrying a parameter `Accept` ignores.
    pub(super) struct Tally;

    impl BodyCodec<u64> for Tally {
        const MEDIA_TYPE: &'static str = "application/vnd.example.tally; base=10";

        fn decode(_: &bytes::Bytes) -> Result<u64, DecodeError> {
            unreachable!("only offered")
        }

        fn encode(value: &u64, out: &mut impl std::io::Write) -> std::io::Result<()> {
            write!(out, "{value}")
        }
    }

    #[test]
    fn is_described_under_its_media_type_over_the_schema_of_its_value() {
        let described = Encoded::<Tally, u64>::responses(&mut Registry::default());
        let Some(kynos_openapi::RefOr::Item(ok)) = described.get(200) else {
            panic!("no inline 200 response");
        };

        let media_type = <Tally as BodyCodec<u64>>::MEDIA_TYPE;
        assert_eq!(ok.content.keys().collect::<Vec<_>>(), [media_type]);
        assert_eq!(
            ok.content[media_type].as_item().expect("inline").schema,
            Some(Registry::default().resolve::<u64>())
        );
    }

    #[test]
    fn an_accept_naming_it_chooses_it() {
        type Offer = (Text, Encoded<Tally, u64>);

        for (field, chosen) in [
            ("text/plain", 0),
            ("application/vnd.example.tally", 1),
            ("Application/Vnd.Example.Tally; base=10", 1),
            ("application/*, text/plain;q=0.5", 1),
            ("*/*", 0),
        ] {
            let accepted = Accept::<Offer>::parse(field)
                .expect("valid Accept header")
                .choose::<Offer>()
                .ok();
            assert_eq!(accepted, Some(chosen), "Accept: {field}");
        }
    }
}
