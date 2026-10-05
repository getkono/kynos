//! What the body and query extractors read, what they refuse, and what they
//! describe.
//!
//! [`errors.rs`](errors.rs) pins each extractor's rejection *type*; this runs
//! the extractors whose decision is made from more than one input — a body
//! chosen by `Content-Type`, a body that may be absent, a media type carrying
//! parameters, and a whole query string read as one document — and holds each
//! decision against the description the same type writes. Every extractor is
//! driven through the public trait a handler's arguments are read with, so the
//! rejection compared is the exact value, `received` included, rather than the
//! status it renders as.

#![cfg(all(feature = "macros", feature = "json", feature = "form"))]

use std::collections::BTreeMap;

use kynos::{
    Router, Schema,
    error::rejection::BodyRejection,
    extract::{
        FromRequest,
        body::{OneOf, form::Form, json::Json, text::Text},
    },
    http::{HeaderValue, Request, StatusCode, body::Body, header},
    openapi::model::paths::operation::Operation,
    response::status::NoContent,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// The JSON document the bodies and the query string below decode.
#[derive(Debug, PartialEq, Schema, Deserialize)]
struct Payload {
    limit: u32,
}

/// A request carrying `body`, declaring `content_type` when there is one.
fn request(content_type: Option<&str>, body: &'static [u8]) -> Request {
    let mut request = Request::new(Body::from_bytes(bytes::Bytes::from_static(body)));
    if let Some(content_type) = content_type {
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(content_type).expect("a valid header value"),
        );
    }
    request
}

/// Reads `T` from a request, the way a handler's last argument is read.
async fn read<T: FromRequest<()>>(
    content_type: Option<&str>,
    body: &'static [u8],
) -> Result<T, T::Rejection> {
    T::from_request(request(content_type, body), &()).await
}

/// The `post` operation the router describes at `path`.
fn operation(router: &Router<()>, path: &str) -> Operation {
    let document = router.openapi().expect("a describable router");
    let item = document
        .paths
        .items
        .get(path)
        .unwrap_or_else(|| panic!("`{path}` is described"));
    item.post
        .as_deref()
        .cloned()
        .unwrap_or_else(|| panic!("`{path}` describes a `post`"))
}

/// The request body `operation` describes, as the JSON a reader of the
/// document sees.
fn request_body(operation: &Operation) -> Value {
    serde_json::to_value(
        operation
            .request_body
            .as_ref()
            .expect("the operation describes a request body"),
    )
    .expect("a request body serializes")
}

/// Asserts that `outcome` is the 415 quoting exactly `received`.
fn assert_unsupported<T: std::fmt::Debug>(
    outcome: Result<T, BodyRejection>,
    received: Option<&str>,
) {
    match outcome {
        Err(BodyRejection::UnsupportedMediaType { received: quoted }) => {
            assert_eq!(quoted.as_deref(), received, "the 415 quotes what was sent");
        }
        other => panic!("expected a 415 quoting {received:?}, got {other:?}"),
    }
}

mod content_type {
    use super::{BTreeMap, BodyRejection, Form, Json, Text, assert_unsupported, read};

    /// One parameter list a client may append to a codec's media type, and
    /// whether every codec accepts it.
    ///
    /// The cases are the axes `parameters_are_acceptable` decides on — none,
    /// the one parameter it admits, that parameter's spelling, its value, and
    /// a parameter beside it — plus the spellings the media-type grammar of
    /// RFC 9110 section 8.3.1 permits around them.
    const PARAMETERS: &[(&str, bool)] = &[
        ("", true),
        (";charset=utf-8", true),
        ("; charset=utf-8", true),
        (" ; charset=utf-8", true),
        ("; charset=\"utf-8\"", true),
        ("; CHARSET=UTF-8", true),
        ("; charset=UTF-8", true),
        // RFC 9110 permits an empty parameter after a `;`.
        (";", true),
        ("; charset=utf-8;", true),
        ("; charset=iso-8859-1", false),
        ("; charset=utf-16", false),
        ("; charset=utf-8; version=1", false),
        ("; version=1", false),
        ("; charset", false),
    ];

    /// Reads one acceptable body through the codec for `media_type`.
    async fn accepts(media_type: &str, content_type: &str) -> Result<(), BodyRejection> {
        match media_type {
            "application/json" => read::<Json<u32>>(Some(content_type), b"1")
                .await
                .map(|Json(value)| assert_eq!(value, 1)),
            "application/x-www-form-urlencoded" => {
                read::<Form<BTreeMap<String, String>>>(Some(content_type), b"a=1")
                    .await
                    .map(|Form(pairs)| assert_eq!(pairs["a"], "1"))
            }
            "text/plain" => read::<Text>(Some(content_type), b"x")
                .await
                .map(|Text(text)| assert_eq!(text, "x")),
            other => unreachable!("no codec here reads `{other}`"),
        }
    }

    /// Every codec reading text accepts its media type bare or with a UTF-8
    /// charset, in any case and either quoting, and refuses everything else
    /// with a 415 that quotes the header it was sent.
    #[tokio::test]
    async fn a_body_content_type_is_accepted_bare_or_with_a_utf_8_charset_and_nothing_else() {
        for media_type in [
            "application/json",
            "application/x-www-form-urlencoded",
            "text/plain",
        ] {
            for spelling in [media_type.to_owned(), media_type.to_ascii_uppercase()] {
                for &(parameters, accepted) in PARAMETERS {
                    let content_type = format!("{spelling}{parameters}");
                    let outcome = accepts(media_type, &content_type).await;
                    if accepted {
                        assert!(
                            outcome.is_ok(),
                            "`{content_type}` is accepted, got {outcome:?}"
                        );
                    } else {
                        assert_unsupported(outcome, Some(&content_type));
                    }
                }
            }

            // No header at all is the same 415, quoting nothing.
            let outcome = match media_type {
                "application/json" => read::<Json<u32>>(None, b"1").await.map(drop),
                "application/x-www-form-urlencoded" => {
                    read::<Form<BTreeMap<String, String>>>(None, b"a=1")
                        .await
                        .map(drop)
                }
                _ => read::<Text>(None, b"x").await.map(drop),
            };
            assert_unsupported(outcome, None);
        }
    }

    /// A structured syntax suffix is not the media type it extends: a body
    /// is accepted under what the description claims and nothing broader.
    #[tokio::test]
    async fn a_structured_suffix_is_not_the_media_type_it_extends() {
        assert_unsupported(
            read::<Json<u32>>(Some("application/vnd.acme+json"), b"1").await,
            Some("application/vnd.acme+json"),
        );
    }

    /// Form syntax admits no malformed input, so a pair that does not fit
    /// the type is the only way a form body fails — and that is a 422 keyed
    /// by the root pointer.
    #[tokio::test]
    async fn a_form_body_that_does_not_fit_is_422() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Page {
            page: u32,
        }

        let rejection =
            read::<Form<Page>>(Some("application/x-www-form-urlencoded"), b"page=first")
                .await
                .expect_err("`first` is not a `u32`");

        assert_eq!(
            rejection.status(),
            kynos::http::StatusCode::UNPROCESSABLE_ENTITY
        );
        let BodyRejection::Schema { failures } = rejection else {
            panic!("expected a schema failure, got {rejection:?}");
        };
        assert_eq!(
            failures.keys().collect::<Vec<_>>(),
            [""],
            "attributed to the root"
        );
    }

    /// A `text/plain` body is read as UTF-8, so octets that are not are a
    /// body that does not say what it claims: a 400, not a lossy string.
    #[tokio::test]
    async fn a_text_body_that_is_not_utf_8_is_400() {
        let rejection = read::<Text>(Some("text/plain"), b"caf\xe9")
            .await
            .expect_err("a lone 0xE9 is not UTF-8");

        assert_eq!(rejection.status(), kynos::http::StatusCode::BAD_REQUEST);
        assert!(
            matches!(rejection, BodyRejection::Syntax { .. }),
            "expected a syntax failure, got {rejection:?}"
        );
    }
}

mod one_of {
    use super::{
        BodyRejection, Json, NoContent, OneOf, Payload, Router, StatusCode, Text,
        assert_unsupported, json, operation, read, request_body,
    };

    #[kynos::post("/either")]
    async fn either(_body: OneOf<Json<Payload>, Text>) -> NoContent {
        NoContent
    }

    /// Each side is reached by its own media type, and only by it.
    #[tokio::test]
    async fn a_one_of_body_is_read_as_the_side_its_content_type_names() {
        let left = read::<OneOf<Json<Payload>, Text>>(Some("application/json"), br#"{"limit":3}"#)
            .await
            .expect("a JSON body is the left side");
        assert_eq!(left, OneOf::Left(Json(Payload { limit: 3 })));

        let right = read::<OneOf<Json<Payload>, Text>>(Some("text/plain"), br#"{"limit":3}"#)
            .await
            .expect("a text body is the right side");
        assert_eq!(right, OneOf::Right(Text(r#"{"limit":3}"#.to_owned())));
    }

    /// The side is chosen from the head before either reads a byte, so a body
    /// the chosen side cannot read is that side's rejection — even when the
    /// other side would have read the same bytes.
    #[tokio::test]
    async fn a_malformed_left_body_fails_as_left_rather_than_falling_through() {
        let rejection = read::<OneOf<Json<Payload>, Text>>(Some("application/json"), b"{")
            .await
            .expect_err("`{` is not JSON");
        assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
        assert!(
            matches!(rejection, BodyRejection::Syntax { .. }),
            "expected the JSON side's syntax failure, got {rejection:?}"
        );

        let rejection = read::<OneOf<Json<Payload>, Text>>(Some("application/json"), b"{}")
            .await
            .expect_err("`{}` lacks `limit`");
        assert_eq!(rejection.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// A media type neither side offers is a 415 quoting it, and so is none.
    #[tokio::test]
    async fn a_content_type_neither_side_offers_is_415() {
        assert_unsupported(
            read::<OneOf<Json<Payload>, Text>>(Some("application/xml"), b"<a/>").await,
            Some("application/xml"),
        );
        assert_unsupported(
            read::<OneOf<Json<Payload>, Text>>(Some("text/plain; charset=latin1"), b"x").await,
            Some("text/plain; charset=latin1"),
        );
        assert_unsupported(read::<OneOf<Json<Payload>, Text>>(None, b"x").await, None);
    }

    /// One required body carrying both representations, each described as
    /// its own side describes it.
    #[test]
    fn a_one_of_body_describes_both_media_types() {
        let operation = operation(
            &Router::<()>::new().mount(kynos::routes![either]),
            "/either",
        );

        assert_eq!(
            request_body(&operation),
            json!({
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/Payload" }
                    },
                    "text/plain": {
                        "schema": { "type": "string" }
                    }
                },
                "required": true
            })
        );
    }
}

mod optional {
    use super::{
        BodyRejection, Json, NoContent, Payload, Router, StatusCode, Text, assert_unsupported,
        json, operation, read, request_body,
    };

    #[kynos::post("/maybe")]
    async fn maybe(_body: Option<Json<Payload>>) -> NoContent {
        NoContent
    }

    #[kynos::post("/always")]
    async fn always(_body: Json<Payload>) -> NoContent {
        NoContent
    }

    /// Absence is read from the head alone: no `Content-Type` is no body
    /// whatever bytes follow, and a `Content-Type` is a body `T` must read.
    #[tokio::test]
    async fn an_optional_body_is_absent_only_without_a_content_type() {
        assert_eq!(read::<Option<Text>>(None, b"").await.expect("absent"), None);
        assert_eq!(
            read::<Option<Text>>(None, b"ignored")
                .await
                .expect("absent"),
            None,
            "bytes without a declared media type are not a representation"
        );
        assert_eq!(
            read::<Option<Text>>(Some("text/plain"), b"present")
                .await
                .expect("present"),
            Some(Text("present".to_owned()))
        );

        // An empty JSON body that names its media type is still malformed
        // JSON, not an absent one.
        let rejection = read::<Option<Json<Payload>>>(Some("application/json"), b"")
            .await
            .expect_err("an empty document is not JSON");
        assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
        assert!(
            matches!(rejection, BodyRejection::Syntax { .. }),
            "expected a syntax failure, got {rejection:?}"
        );
    }

    /// Emptiness is not absence: an empty text body is the empty string.
    #[tokio::test]
    async fn an_empty_text_body_with_a_content_type_is_some_empty_string() {
        assert_eq!(
            read::<Option<Text>>(Some("text/plain"), b"")
                .await
                .expect("an empty string is a string"),
            Some(Text(String::new()))
        );
    }

    /// `Option` does not soften the wrapped codec's 415.
    #[tokio::test]
    async fn an_optional_body_of_an_unsupported_type_is_still_415() {
        assert_unsupported(
            read::<Option<Json<Payload>>>(Some("application/xml"), b"<a/>").await,
            Some("application/xml"),
        );
    }

    /// The optional body is the wrapped body with `required: false`, beside a
    /// control showing the bare body is `required: true`.
    #[test]
    fn an_optional_json_body_is_described_as_not_required() {
        let router = Router::<()>::new().mount(kynos::routes![maybe, always]);
        let content = json!({
            "application/json": {
                "schema": { "$ref": "#/components/schemas/Payload" }
            }
        });

        assert_eq!(
            request_body(&operation(&router, "/maybe")),
            json!({ "content": content, "required": false })
        );
        assert_eq!(
            request_body(&operation(&router, "/always")),
            json!({ "content": content, "required": true })
        );
    }
}

#[cfg(feature = "openapi32")]
mod query_string {
    use kynos::{
        error::rejection::QueryRejection,
        extract::{FromRequestParts, params::querystring::QueryString},
        http::{Parts, Uri, media},
    };

    use super::{Body, NoContent, Payload, Request, Router, Schema, StatusCode, json, operation};

    /// A query whose fields are all optional, so the empty object is one.
    #[derive(Debug, PartialEq, Schema, serde::Deserialize)]
    struct Filter {
        limit: Option<u32>,
    }

    #[kynos::post("/search")]
    async fn search(_query: QueryString<Payload, media::Json>) -> NoContent {
        NoContent
    }

    /// The head of a request for `uri`.
    fn parts(uri: &'static str) -> Parts {
        let mut request = Request::new(Body::empty());
        *request.uri_mut() = Uri::from_static(uri);
        request.into_parts().0
    }

    /// Reads the whole query string of `uri` as `T`, declared as `M`.
    async fn read<T, M>(uri: &'static str) -> Result<QueryString<T, M>, QueryRejection>
    where
        QueryString<T, M>: FromRequestParts<(), Rejection = QueryRejection>,
    {
        QueryString::<T, M>::from_request_parts(&mut parts(uri), &()).await
    }

    /// The parameter name a refusal carries.
    fn named(rejection: &QueryRejection) -> &str {
        match rejection {
            QueryRejection::Invalid { name, .. } => name,
            other => panic!("expected an invalid parameter, got {other:?}"),
        }
    }

    /// Asserts that `outcome` is the 400 naming the `querystring` parameter.
    fn assert_refused<T: std::fmt::Debug>(outcome: Result<T, QueryRejection>) {
        let rejection = outcome.expect_err("the query string is refused");
        assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
        assert_eq!(named(&rejection), "querystring");
    }

    /// The query string is percent-decoded once and then read as one JSON
    /// document.
    #[tokio::test]
    async fn a_query_string_is_percent_decoded_and_read_as_json() {
        let query = read::<Payload, media::Json>("/search?%7B%22limit%22%3A3%7D")
            .await
            .expect("an encoded JSON document");
        assert_eq!(query.into_inner(), Payload { limit: 3 });
    }

    /// No `?` at all reads exactly as a bare `?` does: as the empty
    /// document, which is not JSON — so even a type every field of which is
    /// optional, and an `Option` of one, is refused.
    #[tokio::test]
    async fn an_absent_query_string_is_the_empty_document() {
        for (absent, empty) in [
            (
                format!("{:?}", read::<Filter, media::Json>("/search").await),
                format!("{:?}", read::<Filter, media::Json>("/search?").await),
            ),
            (
                format!("{:?}", read::<Option<Filter>, media::Json>("/search").await),
                format!(
                    "{:?}",
                    read::<Option<Filter>, media::Json>("/search?").await
                ),
            ),
        ] {
            assert_eq!(absent, empty, "absence is the empty query string");
        }

        assert_refused(read::<Filter, media::Json>("/search").await);
        assert_refused(read::<Option<Filter>, media::Json>("/search").await);

        // The control: the empty object is the document those fields allow.
        assert_eq!(
            read::<Filter, media::Json>("/search?%7B%7D")
                .await
                .expect("`{}` leaves every field absent")
                .into_inner(),
            Filter { limit: None }
        );
    }

    /// A marker naming a media type that is not JSON has no decoder, and is
    /// refused before the query string is read — even one that is valid
    /// JSON.
    #[tokio::test]
    async fn a_non_json_marker_is_refused_with_400_naming_querystring() {
        assert_refused(read::<Payload, media::Pdf>("/search?%7B%22limit%22%3A3%7D").await);
        assert_refused(read::<Payload, media::OctetStream>("/search?%7B%22limit%22%3A3%7D").await);
    }

    /// Every way a JSON query string fails to decode is the same 400, under
    /// the same name: octets that are not UTF-8, text that is not JSON, and
    /// JSON that does not fit the type.
    #[tokio::test]
    async fn an_undecodable_query_string_is_refused_with_400_naming_querystring() {
        assert_refused(read::<Payload, media::Json>("/search?%FF").await);
        assert_refused(read::<Payload, media::Json>("/search?%7B").await);
        assert_refused(read::<Payload, media::Json>("/search?%7B%7D").await);
    }

    /// One `in: querystring` parameter, carrying its schema under the media
    /// type its marker names — and nothing else on the operation.
    #[test]
    fn a_query_string_is_described_as_one_querystring_parameter_with_its_media_type() {
        let operation = operation(
            &Router::<()>::new().mount(kynos::routes![search]),
            "/search",
        );

        assert_eq!(
            serde_json::to_value(&operation.parameters).expect("parameters serialize"),
            json!([{
                "name": "querystring",
                "in": "querystring",
                "content": {
                    "application/json": {
                        "schema": { "$ref": "#/components/schemas/Payload" }
                    }
                }
            }])
        );
    }
}
