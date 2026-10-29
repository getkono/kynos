use std::collections::HashSet;

use kynos_openapi::{Document, Operation, PathItem, PathTemplate, RefOr};

use super::{conformance, declared_response, matched_template, template_matches};

/// The segments a template may be built from: literals, a whole-segment
/// variable, and a variable behind a literal prefix or before a literal
/// suffix, the two mixed forms the router serves. The variable is a marker
/// rather than a name: a template may not repeat one, so each is numbered
/// by position before the template is used.
const TEMPLATE_ALPHABET: [&str; 5] = ["a", "b", "{}", "v{}", "{}.txt"];
/// The segments a concrete path may be built from. `""` is here because a
/// double slash is a path a client can send and a variable must not accept;
/// `v` and `.txt` are a mixed segment's literal with nothing left for its
/// variable, which it must not accept either.
const PATH_ALPHABET: [&str; 9] = ["a", "b", "x", "", "v", "va", ".txt", "a.txt", "va.txt"];
/// What a variable stands for: every non-empty path segment. A mixed
/// segment's variable stands for part of one, and each part this alphabet
/// can leave it (`a` of `va`, `va` of `va.txt`, `a.txt` of `va.txt`) is a
/// segment here too, so the substitution is total over the paths swept.
const SUBSTITUTIONS: [&str; 8] = ["a", "b", "x", "v", "va", ".txt", "a.txt", "va.txt"];

/// Every sequence of `1..=3` segments over `alphabet`, as a rooted path.
fn rooted(alphabet: &[&str]) -> Vec<String> {
    let mut all = Vec::new();
    for length in 1..=3 {
        let mut sequences: Vec<Vec<&str>> = vec![Vec::new()];
        for _ in 0..length {
            sequences = sequences
                .into_iter()
                .flat_map(|prefix| {
                    alphabet.iter().map(move |segment| {
                        let mut next = prefix.clone();
                        next.push(segment);
                        next
                    })
                })
                .collect();
        }
        all.extend(
            sequences
                .into_iter()
                .map(|parts| format!("/{}", parts.join("/"))),
        );
    }
    all
}

/// Numbers each `{}` marker by position, so no template repeats a variable
/// name — which `PathTemplate` refuses, and rightly.
fn named(template: &str) -> String {
    let mut next = 0;
    template
        .split('/')
        .map(|segment| {
            if segment.contains("{}") {
                next += 1;
                segment.replace("{}", &format!("{{v{}}}", next - 1))
            } else {
                segment.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Every concrete path a template accepts, built by *substitution*.
///
/// The independently constructed oracle the parser rule asks for: it
/// expands where `template_matches` compares, so the two agree only where
/// both are right. An oracle that walked the segments the same way would
/// agree with the matcher by construction, including wherever both were
/// wrong.
fn instances(template: &str) -> HashSet<String> {
    let mut expanded = vec![String::new()];

    for segment in template.trim_start_matches('/').split('/') {
        // The sweep's segments hold at most one expression.
        let substituted: Vec<String> = match segment.split_once('{') {
            Some((before, rest)) => {
                let (_, after) = rest.split_once('}').expect("a closed expression");
                SUBSTITUTIONS
                    .iter()
                    .map(|value| format!("{before}{value}{after}"))
                    .collect()
            }
            None => vec![segment.to_owned()],
        };
        expanded = expanded
            .iter()
            .flat_map(|prefix| {
                substituted
                    .iter()
                    .map(move |value| format!("{prefix}/{value}"))
            })
            .collect();
    }

    expanded.into_iter().collect()
}

/// A total sweep rather than a property.
///
/// The input space is finite and small — 155 templates against 819 paths —
/// and `docs/testing.md` says a sweep is the stronger statement there,
/// because it is total where a draw from the same space is a sample of it.
/// It is also what keeps `proptest` out of `crates/kynos`'s
/// dev-dependencies, which matters: five UI snapshots embed rustc's "the
/// following other types implement" list.
#[test]
fn a_template_matches_exactly_the_paths_substituting_into_it_produces() {
    let templates: Vec<String> = rooted(&TEMPLATE_ALPHABET)
        .iter()
        .map(|t| named(t))
        .collect();
    let paths = rooted(&PATH_ALPHABET);

    assert_eq!(templates.len(), 155);
    assert_eq!(paths.len(), 819);

    for template in &templates {
        let accepted = instances(template);

        for path in &paths {
            let expected = accepted.contains(path);
            assert_eq!(
                template_matches(template, path),
                expected,
                "`{template}` against `{path}`"
            );
        }
    }
}

/// Every template in the sweep is one the model would accept, so the sweep
/// is over the language `PathTemplate` defines rather than over strings
/// that only look like it.
#[test]
fn every_template_in_the_sweep_is_one_the_model_parses() {
    for template in rooted(&TEMPLATE_ALPHABET).iter().map(|t| named(t)) {
        assert!(
            PathTemplate::parse(&template).is_ok(),
            "`{template}` is not a path template"
        );
    }
}

/// A segment holding two expressions, which the model accepts and the router
/// refuses, gives each a non-empty run of its own, with the literal between
/// them matched where it first fits.
#[test]
fn each_expression_in_a_crowded_segment_stands_for_a_non_empty_run() {
    let template = "/files/{name}-{version}";
    assert!(PathTemplate::parse(template).is_ok());

    for path in ["/files/a-b", "/files/a-b-c", "/files/a--b"] {
        assert!(
            template_matches(template, path),
            "`{template}` against `{path}`"
        );
    }
    for path in ["/files/ab", "/files/a-", "/files/-b", "/files/-", "/files/"] {
        assert!(
            !template_matches(template, path),
            "`{template}` against `{path}`"
        );
    }
}

fn document(templates: &[&str]) -> Document {
    let mut document = Document::new(
        kynos_openapi::SpecVersion::V3_1,
        kynos_openapi::Info::new("Fixture", "1.0.0"),
    );
    for template in templates {
        document.paths.items.insert(
            (*template).to_owned(),
            PathItem {
                get: Some(Box::new(Operation::default())),
                ..PathItem::default()
            },
        );
    }
    document
}

/// A literal beats a variable in the same position, which is what keeps a
/// declared `/users/me` from being answered by `/users/{id}`'s description.
#[test]
fn a_literal_beats_a_variable_in_the_same_segment() {
    let document = document(&["/users/{id}", "/users/me"]);

    assert_eq!(matched_template(&document, "/users/me"), Some("/users/me"));
    assert_eq!(
        matched_template(&document, "/users/42"),
        Some("/users/{id}")
    );
}

/// The router ranks a segment mixing an expression with literal text between
/// a literal and a whole-segment variable, and a longer literal part above a
/// shorter one. Both document orders, so no case is a tie broken by position.
#[test]
fn a_mixed_segment_ranks_between_a_literal_and_a_variable() {
    let cases = [
        (
            ["/files/{id}", "/files/{path}.txt"],
            "/files/a.txt",
            "/files/{path}.txt",
        ),
        (
            ["/files/{id}", "/files/{path}.txt"],
            "/files/a",
            "/files/{id}",
        ),
        (
            ["/files/{id}", "/files/v{name}"],
            "/files/v1",
            "/files/v{name}",
        ),
        (
            ["/files/a.txt", "/files/{path}.txt"],
            "/files/a.txt",
            "/files/a.txt",
        ),
        (
            ["/files/{a}.txt", "/files/{b}.tar.txt"],
            "/files/x.tar.txt",
            "/files/{b}.tar.txt",
        ),
        (
            ["/files/v{a}", "/files/va{b}"],
            "/files/vax",
            "/files/va{b}",
        ),
    ];

    for (templates, path, expected) in cases {
        for templates in [templates, [templates[1], templates[0]]] {
            assert_eq!(
                matched_template(&document(&templates), path),
                Some(expected),
                "{templates:?} against `{path}`"
            );
        }
    }
}

/// The router prefers a literal segment at the first position where two
/// matching templates differ, left to right, so `/a/{x}/{y}` answers `/a/b/c`
/// although `/{x}/b/c` holds more literals. Both document orders, so the
/// choice is not a tie broken by position.
#[test]
fn the_first_differing_segment_decides_between_matching_templates() {
    for templates in [["/a/{x}/{y}", "/{x}/b/c"], ["/{x}/b/c", "/a/{x}/{y}"]] {
        let document = document(&templates);

        assert_eq!(matched_template(&document, "/a/b/c"), Some("/a/{x}/{y}"));
        assert_eq!(matched_template(&document, "/z/b/c"), Some("/{x}/b/c"));
    }
}

/// A request target carries a query and may carry a fragment; a `paths` key
/// carries neither, so both are cut before matching.
#[test]
fn a_query_or_fragment_is_not_part_of_what_is_matched() {
    let document = document(&["/users"]);

    assert_eq!(
        matched_template(&document, "/users?limit=2"),
        Some("/users")
    );
    assert_eq!(matched_template(&document, "/users#top"), Some("/users"));
    assert_eq!(matched_template(&document, "/other"), None);
}

/// The precedence a consumer resolves a status by: exact, then wildcard,
/// then `default`. All three, and the miss.
#[test]
fn a_status_resolves_to_the_most_specific_key_declaring_it() {
    use kynos_openapi::{RefOr, Response, Responses};

    let responses = Responses::new()
        .with(200, Response::new("exact"))
        .with_pattern(
            kynos_openapi::StatusPattern::Success,
            RefOr::Item(Response::new("wildcard")),
        );

    assert_eq!(
        declared_response(&responses, 200).map(|(key, _)| key),
        Some("200".to_owned())
    );
    assert_eq!(
        declared_response(&responses, 204).map(|(key, _)| key),
        Some("2XX".to_owned())
    );
    assert_eq!(declared_response(&responses, 404).map(|(key, _)| key), None);

    let mut with_default = responses;
    with_default.default_response = Some(RefOr::Item(Response::new("default")));
    assert_eq!(
        declared_response(&with_default, 404).map(|(key, _)| key),
        Some("default".to_owned())
    );
}

/// One `content` map, from the media types it declares.
pub(super) fn content(declared: &[&str]) -> kynos_openapi::Map<RefOr<kynos_openapi::MediaType>> {
    declared
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                kynos_openapi::MediaType::new(kynos_openapi::Schema::Object(Box::default())).into(),
            )
        })
        .collect()
}

/// A document declaring one `GET /thing` operation with `responses`.
///
/// The sibling of [`document`] for the cases that are about what a response
/// says rather than about which path matched: those need a `Responses` value
/// under the operation, which `document`'s default `Operation` leaves empty.
fn document_declaring(responses: kynos_openapi::Responses) -> Document {
    let mut document = document(&["/thing"]);
    document
        .paths
        .items
        .get_mut("/thing")
        .expect("the template just inserted")
        .get
        .as_mut()
        .expect("the operation just inserted")
        .responses = responses;
    document
}

/// One `GET /thing` exchange, as the recorder would have stored it.
fn observed(status: u16, content_type: Option<&str>, body: &str) -> crate::test::Observed {
    let mut headers = crate::http::HeaderMap::new();
    if let Some(content_type) = content_type {
        headers.insert(
            crate::http::header::CONTENT_TYPE,
            crate::http::HeaderValue::from_str(content_type).expect("a media type"),
        );
    }

    crate::test::Observed {
        method: crate::http::Method::GET,
        path: "/thing".to_owned(),
        status: crate::http::StatusCode::from_u16(status).expect("a status"),
        headers,
        body: bytes::Bytes::copy_from_slice(body.as_bytes()),
    }
}

/// A response declaring no content is a claim that none was sent, so a body
/// arriving under one is a description that lies about the exchange.
///
/// This is the defect issue #104 reported at the 429: eight of the ten
/// `ShortCircuit` implementations put a problem document on the wire under a
/// description declaring nothing, and the harness read "declares nothing" as
/// "nothing to check".
///
/// The second assertion names the arm rather than the prefix the three arms
/// share, which sits outside the match: on the prefix alone, collapsing all
/// three arms into one sentence keeps every case here green, and a report
/// that cannot say which shape arrived is the half of the diagnostic a reader
/// acts on.
#[test]
fn a_body_sent_under_a_response_declaring_no_content_is_reported() {
    let document = document_declaring(
        kynos_openapi::Responses::new().with(429, kynos_openapi::Response::new("rate limited")),
    );
    let record = observed(
        429,
        Some("application/problem+json"),
        r#"{"type":"about:blank","status":429}"#,
    );

    let reasons = conformance(&document, &record);

    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].contains("declares no content"), "{}", reasons[0]);
    assert!(
        reasons[0].contains("a 35-byte `application/problem+json` body"),
        "{}",
        reasons[0]
    );
}

/// The second half of the disjunction: a media type with no octets behind it.
///
/// A `Content-Type` names the representation a client should parse, so a head
/// carrying one under a declaration of none is the same lie a body would be —
/// and a length check alone would never reach it.
///
/// This is the only one of the three arms that reports no byte count, which is
/// what tells a reader the octets were the half that never arrived.
#[test]
fn a_head_sent_under_a_response_declaring_no_content_is_reported() {
    let document = document_declaring(
        kynos_openapi::Responses::new().with(429, kynos_openapi::Response::new("rate limited")),
    );
    let record = observed(429, Some("application/problem+json"), "");

    let reasons = conformance(&document, &record);

    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].contains("declares no content"), "{}", reasons[0]);
    assert!(
        reasons[0].contains("a `application/problem+json` head with no body"),
        "{}",
        reasons[0]
    );
}

/// The first half: octets with no media type at all.
///
/// A media-type comparison would find nothing to compare and pass, so the
/// predicate reads the body's length as well as the header.
///
/// It shares its byte count with the arm above, so the fragment that separates
/// the two is the missing media type rather than the count.
#[test]
fn a_body_with_no_media_type_under_a_declaration_of_none_is_reported() {
    let document = document_declaring(
        kynos_openapi::Responses::new().with(500, kynos_openapi::Response::new("it went wrong")),
    );
    let record = observed(500, None, "something");

    let reasons = conformance(&document, &record);

    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].contains("declares no content"), "{}", reasons[0]);
    assert!(
        reasons[0].contains("a 9-byte body with no `Content-Type`"),
        "{}",
        reasons[0]
    );
}

/// The control: a response that sends nothing under a declaration of nothing
/// still conforms, and must go on doing so.
///
/// Kynos ships six shapes that legitimately send neither octets nor a
/// `Content-Type`: `NoContent`'s 204, every `Redirect<CODE>`, the conditional
/// 304 whose replayed field list deliberately omits `Content-Type`, the ranged
/// 304 that guards its media type behind the status, the asset 304 whose header
/// set is `etag`/`cache-control`/`content-encoding`/`vary`, and a HEAD against
/// a status declaring no representation. One case covers all six because they
/// differ only in status and in headers the predicate does not read: in the two
/// facts it does read — no body, no media type — they are the same exchange.
///
/// The CORS preflight 204 was counted here and is not one of them: it sends
/// neither, but it is never described, so nothing declares the response this
/// branch reads and the exchange cannot arrive. The HEAD is one of them and is
/// exercised nowhere else — no test drives a HEAD through
/// `assert_conformance`, so this case standing in for it is the whole of what
/// holds it.
#[test]
fn a_response_that_sends_nothing_and_declares_nothing_conforms() {
    let document = document_declaring(
        kynos_openapi::Responses::new()
            .with(204, kynos_openapi::Response::new("nothing to return")),
    );
    let record = observed(204, None, "");

    assert!(conformance(&document, &record).is_empty());
}

/// A 200 declaring an `application/json` body.
fn json_ok() -> kynos_openapi::Responses {
    let mut ok = kynos_openapi::Response::new("the thing");
    ok.content = content(&["application/json"]);
    kynos_openapi::Responses::new().with(200, ok)
}

/// One `HEAD /thing` exchange: the fields a GET would send, and no content.
fn observed_head(status: u16, content_type: Option<&str>) -> crate::test::Observed {
    crate::test::Observed {
        method: crate::http::Method::HEAD,
        ..observed(status, content_type, "")
    }
}

/// The router answers a HEAD on a path declaring `get` and no `head` from the
/// GET operation, so that is the operation the exchange is read against -- and
/// a harness reading it against a `head` nobody declared would report Kynos's
/// own behaviour as a violation.
#[test]
fn a_head_is_checked_against_the_get_operation_it_was_answered_by() {
    let document = document_declaring(json_ok());

    let reasons = conformance(&document, &observed_head(200, Some("application/json")));

    assert!(reasons.is_empty(), "{reasons:?}");
}

/// RFC 9110 section 9.3.2: no response to a HEAD carries content, whatever the
/// operation declares. The fields are still held to the declaration.
#[test]
fn a_head_declares_content_it_never_sends() {
    let mut document = document_declaring(kynos_openapi::Responses::new());
    document
        .paths
        .items
        .get_mut("/thing")
        .expect("the template just inserted")
        .head = Some(Box::new(Operation {
        responses: json_ok(),
        ..Operation::default()
    }));

    let reasons = conformance(&document, &observed_head(200, Some("application/json")));
    assert!(reasons.is_empty(), "{reasons:?}");

    let reasons = conformance(&document, &observed_head(200, Some("text/plain")));
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(
        reasons[0].contains("`text/plain` is not a declared representation"),
        "{}",
        reasons[0]
    );

    // A schema that accepts anything, and octets that would satisfy it: the
    // octets are still the violation.
    let carrying = crate::test::Observed {
        method: crate::http::Method::HEAD,
        ..observed(200, Some("application/json"), "{}")
    };
    let reasons = conformance(&document, &carrying);
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(
        reasons[0].contains("a 2-byte body was sent in response to a HEAD"),
        "{}",
        reasons[0]
    );
}

/// A 200 whose every declared representation carries the `false` schema, which
/// no instance satisfies: a body is reported exactly when it is validated.
fn refusing_ok(media_types: &[&str]) -> kynos_openapi::Responses {
    let mut ok = kynos_openapi::Response::new("the thing");
    ok.content = media_types
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                kynos_openapi::MediaType::new(kynos_openapi::Schema::Bool(false)).into(),
            )
        })
        .collect();
    kynos_openapi::Responses::new().with(200, ok)
}

/// Only a JSON-based body is held to its schema: `application/json` itself and
/// any `+json` structured syntax suffix (RFC 6839). Any other media type has no
/// decoder here, so its schema asserts nothing rather than something wrong.
#[test]
fn only_a_json_based_body_is_validated_against_its_schema() {
    let document = document_declaring(refusing_ok(&[
        "application/json",
        "application/problem+json",
        "text/plain",
    ]));

    for media_type in ["application/json", "application/problem+json"] {
        let reasons = conformance(&document, &observed(200, Some(media_type), "{}"));
        assert_eq!(reasons.len(), 1, "{media_type}: {reasons:?}");
        assert!(
            reasons[0].contains("the body does not match the declared schema"),
            "{media_type}: {}",
            reasons[0]
        );
    }

    let reasons = conformance(&document, &observed(200, Some("text/plain"), "{}"));
    assert!(reasons.is_empty(), "{reasons:?}");
}

/// A response under a media-range key conforms, and is held to that key's
/// schema: whether a body is validated follows the media type actually sent,
/// not the range that declared it.
#[test]
fn a_body_under_a_media_range_key_is_checked_against_that_key() {
    let document = document_declaring(refusing_ok(&["image/*", "*/*"]));

    let reasons = conformance(&document, &observed(200, Some("image/png"), "\u{89}PNG"));
    assert!(reasons.is_empty(), "{reasons:?}");

    let reasons = conformance(&document, &observed(200, Some("application/json"), "{}"));
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(
        reasons[0].contains("the body does not match the declared schema"),
        "{}",
        reasons[0]
    );
}

/// A representation given by a `#/components/mediaTypes` `$ref` is held to the
/// schema of the media type it names, as an inline one is to its own.
#[cfg(feature = "openapi32")]
#[test]
fn a_representation_by_reference_is_held_to_the_schema_it_names() {
    use kynos_openapi::{MediaType, Ref, Response, Responses, Schema};

    let mut ok = Response::new("the thing");
    ok.content.insert(
        "application/json".to_owned(),
        RefOr::Ref(Ref::new("#/components/mediaTypes/Refusing")),
    );
    let mut document = document_declaring(Responses::new().with(200, ok));
    document.components.media_types.insert(
        "Refusing".to_owned(),
        MediaType::new(Schema::Bool(false)).into(),
    );

    let reasons = conformance(&document, &observed(200, Some("application/json"), "{}"));
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(
        reasons[0].contains("the body does not match the declared schema"),
        "{}",
        reasons[0]
    );
}

/// Every reason `conformance` gives that names the exchange rather than the
/// body's content, word for word: the sentence is what a failing suite prints,
/// so a reworded one is a changed contract.
#[test]
fn every_reason_is_reported_in_its_own_sentence() {
    use kynos_openapi::{Ref, RefOr, Responses};

    let mut unresolved = Responses::new();
    unresolved
        .responses
        .insert("200".to_owned(), RefOr::Ref(Ref::response("Missing")));
    let mut unresolved_representation = kynos_openapi::Response::new("the thing");
    unresolved_representation.content.insert(
        "application/json".to_owned(),
        RefOr::Ref(Ref::new("#/components/mediaTypes/Missing")),
    );

    let posted = crate::test::Observed {
        method: crate::http::Method::POST,
        ..observed(200, None, "")
    };
    let purged = crate::test::Observed {
        method: crate::http::Method::from_bytes(b"PURGE").expect("a method token"),
        ..observed(200, None, "")
    };
    let elsewhere = crate::test::Observed {
        path: "/elsewhere".to_owned(),
        ..observed(200, None, "")
    };

    let not_json = serde_json::from_slice::<serde_json::Value>(b"not json")
        .expect_err("not JSON")
        .to_string();

    let cases: Vec<(&str, Document, crate::test::Observed, Vec<String>)> = vec![
        (
            "no path",
            document_declaring(json_ok()),
            elsewhere,
            vec!["no declared path matches this request".to_owned()],
        ),
        (
            "an undeclarable method",
            document_declaring(json_ok()),
            purged,
            vec!["`PURGE` is not a method a description can declare".to_owned()],
        ),
        (
            "no operation",
            document_declaring(json_ok()),
            posted,
            vec!["`/thing` declares no `POST` operation".to_owned()],
        ),
        (
            "an undeclared status",
            document_declaring(json_ok()),
            observed(404, Some("application/json"), "{}"),
            vec!["status 404 is not declared".to_owned()],
        ),
        (
            "an unresolved response `$ref`",
            document_declaring(unresolved),
            observed(200, None, ""),
            vec!["the `200` response resolves to no component".to_owned()],
        ),
        (
            "no `Content-Type`",
            document_declaring(json_ok()),
            observed(200, None, "{}"),
            vec![
                "no `Content-Type` was sent, but the description declares application/json"
                    .to_owned(),
            ],
        ),
        (
            "an undeclared representation",
            document_declaring(json_ok()),
            observed(200, Some("text/plain"), "{}"),
            vec![
                "`text/plain` is not a declared representation; the description declares \
                 application/json"
                    .to_owned(),
            ],
        ),
        (
            "an unresolved representation `$ref`",
            document_declaring(Responses::new().with(200, unresolved_representation)),
            observed(200, Some("application/json"), "{}"),
            vec!["the `application/json` representation resolves to no component".to_owned()],
        ),
        (
            "a body that is not JSON",
            document_declaring(json_ok()),
            observed(200, Some("application/json"), "not json"),
            vec![format!("the body is not valid JSON: {not_json}")],
        ),
    ];

    for (case, document, record, expected) in cases {
        assert_eq!(conformance(&document, &record), expected, "{case}");
    }
}

/// A required header the response did not send is reported by name, whether
/// it is declared inline or behind a `#/components/headers` `$ref`; an
/// optional one is not.
#[test]
fn a_missing_required_header_is_reported_inline_and_by_reference() {
    use kynos_openapi::{Header, Ref, RefOr, Response, Responses, Schema};

    let mut headed = Response::new("the thing");
    let mut required = Header::new(Schema::default());
    required.required = Some(true);
    headed
        .headers
        .insert("X-Inline".to_owned(), RefOr::Item(required.clone()));
    headed.headers.insert(
        "X-Shared".to_owned(),
        RefOr::Ref(Ref::new("#/components/headers/Shared")),
    );
    headed.headers.insert(
        "X-Optional".to_owned(),
        RefOr::Item(Header::new(Schema::default())),
    );
    let mut document = document_declaring(Responses::new().with(200, headed));
    document
        .components
        .headers
        .insert("Shared".to_owned(), RefOr::Item(required));

    assert_eq!(
        conformance(&document, &observed(200, None, "")),
        [
            "the declared required header `X-Inline` was not sent",
            "the declared required header `X-Shared` was not sent",
        ]
    );
}

/// A declared schema the validator cannot compile is reported as the
/// description's fault, in the validator's own words, rather than as a body
/// that failed it.
#[test]
fn a_schema_that_does_not_compile_is_reported_as_such() {
    let mut ok = kynos_openapi::Response::new("the thing");
    let unclosed = kynos_openapi::Schema::Object(Box::new(kynos_openapi::SchemaObject {
        pattern: Some("(".to_owned()),
        ..kynos_openapi::SchemaObject::default()
    }));
    ok.content.insert(
        "application/json".to_owned(),
        kynos_openapi::MediaType::new(unclosed).into(),
    );
    let document = document_declaring(kynos_openapi::Responses::new().with(200, ok));

    let root = serde_json::json!({
        "pattern": "(",
        "components": serde_json::to_value(&document.components).expect("serializable"),
    });
    let error = jsonschema::draft202012::new(&root)
        .expect_err("an unclosed group is no pattern")
        .to_string();

    assert_eq!(
        conformance(
            &document,
            &observed(200, Some("application/json"), r#""text""#)
        ),
        [format!("the declared schema does not compile: {error}")]
    );
}
