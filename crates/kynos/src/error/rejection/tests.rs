use super::{
    AuthRejection, BodyRejection, HeaderRejection, NegotiationRejection, PathRejection,
    QueryRejection, RangeRejection,
};
use crate::{error::problem::IntoProblem, http::StatusCode};

/// Every status a rejection can return at run time has to appear in the set it
/// declares, or the description advertises one thing and the service does
/// another. `statuses()` is written by hand, so this is the assertion that
/// keeps the two halves aligned.
fn declares(observed: &[StatusCode], declared: &[StatusCode]) {
    for status in observed {
        assert!(
            declared.contains(status),
            "{status} is produced but not declared"
        );
    }
}

#[test]
fn a_path_rejection_is_a_bad_request() {
    let rejection = PathRejection::Invalid {
        name: "id".into(),
        detail: "not a number".into(),
    };

    assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
    declares(&[rejection.status()], PathRejection::statuses());
}

#[test]
fn a_query_rejection_is_a_bad_request() {
    let rejection = QueryRejection::Invalid {
        name: "page".into(),
        detail: "not a number".into(),
    };

    assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
    declares(&[rejection.status()], QueryRejection::statuses());
}

#[test]
fn a_header_rejection_is_a_bad_request() {
    let rejection = HeaderRejection::Invalid {
        name: "X-Request-Id".into(),
        detail: "not a uuid".into(),
    };

    assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
    declares(&[rejection.status()], HeaderRejection::statuses());
}

/// The four body failures are four statuses. Syntax and schema are kept apart
/// because only one of them tells a client its serializer is wrong.
#[test]
fn each_body_failure_has_its_own_status() {
    let observed = [
        BodyRejection::Syntax {
            detail: "unexpected end of input".into(),
        },
        BodyRejection::Schema {
            failures: [("/id".to_owned(), "expected integer".to_owned())]
                .into_iter()
                .collect(),
        },
        BodyRejection::UnsupportedMediaType {
            received: Some("text/csv".into()),
        },
        BodyRejection::TooLarge { limit: 1_024 },
    ];

    let statuses: Vec<_> = observed.iter().map(BodyRejection::status).collect();

    assert_eq!(
        statuses,
        [
            StatusCode::BAD_REQUEST,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            StatusCode::PAYLOAD_TOO_LARGE,
        ]
    );
    declares(&statuses, BodyRejection::statuses());
}

/// A schema failure is a set, so it travels as RFC 9457's `errors` extension:
/// one `{pointer, detail}` entry per failing pointer, in pointer order, with
/// the variant's sentence alone as `detail`.
///
/// The failures are inserted out of order, so the array's order is the map's
/// rather than the caller's.
#[test]
fn a_schema_failure_travels_as_one_errors_entry_per_pointer() {
    let problem = BodyRejection::Schema {
        failures: [
            ("/name".to_owned(), "expected a string".to_owned()),
            ("/age".to_owned(), "expected an integer".to_owned()),
        ]
        .into_iter()
        .collect(),
    }
    .into_problem();

    assert_eq!(
        serde_json::to_value(problem).expect("a problem serializes"),
        serde_json::json!({
            "type": "about:blank",
            "title": "Unprocessable Entity",
            "status": 422,
            "detail": "the request body does not satisfy its schema",
            "errors": [
                { "pointer": "/age", "detail": "expected an integer" },
                { "pointer": "/name", "detail": "expected a string" },
            ],
        })
    );
}

/// A query string that breaks a bound travels as a body's schema failure does,
/// at the parameter's own 400, its sentence naming the parameter the pointers
/// read into.
#[test]
fn a_query_schema_failure_travels_as_one_errors_entry_per_pointer() {
    let problem = QueryRejection::Schema {
        name: "querystring".into(),
        failures: [
            ("/limit".to_owned(), "must be at most 100".to_owned()),
            ("/from".to_owned(), "must be at least 1".to_owned()),
        ]
        .into_iter()
        .collect(),
    }
    .into_problem();

    assert_eq!(
        serde_json::to_value(problem).expect("a problem serializes"),
        serde_json::json!({
            "type": "about:blank",
            "title": "Bad Request",
            "status": 400,
            "detail": "query parameter `querystring` does not satisfy its schema",
            "errors": [
                { "pointer": "/from", "detail": "must be at least 1" },
                { "pointer": "/limit", "detail": "must be at most 100" },
            ],
        })
    );
}

/// A 415 names the media type the client sent, or says it sent none, so a
/// client can tell a wrong `Content-Type` from a missing one.
#[test]
fn an_unsupported_media_type_names_what_was_received_or_its_absence() {
    let document = |received: Option<&str>| {
        serde_json::to_value(
            BodyRejection::UnsupportedMediaType {
                received: received.map(ToOwned::to_owned),
            }
            .into_problem(),
        )
        .expect("a problem serializes")
    };

    assert_eq!(
        document(Some("text/csv")),
        serde_json::json!({
            "type": "about:blank",
            "title": "Unsupported Media Type",
            "status": 415,
            "detail": "unsupported media type: `text/csv`",
        })
    );
    assert_eq!(
        document(None),
        serde_json::json!({
            "type": "about:blank",
            "title": "Unsupported Media Type",
            "status": 415,
            "detail": "unsupported media type: the request declared no `Content-Type`",
        })
    );
}

#[test]
fn negotiation_separates_a_bad_header_from_an_unmatchable_one() {
    let observed = [
        NegotiationRejection::MalformedAccept {
            detail: "expected a media range".into(),
        },
        NegotiationRejection::NotAcceptable,
    ];

    let statuses: Vec<_> = observed.iter().map(NegotiationRejection::status).collect();

    assert_eq!(
        statuses,
        [StatusCode::BAD_REQUEST, StatusCode::NOT_ACCEPTABLE]
    );
    declares(&statuses, NegotiationRejection::statuses());
}

/// The one rejection a range can raise, and the length it names.
///
/// RFC 9110 section 15.5.17 asks a 416 to state the current length of the
/// selected representation, and `Problem::into_response` sets no header — so
/// this is the sibling of `only_authentication_declares_a_challenge`: the
/// second rejection whose response is more than a problem document.
#[test]
fn an_unsatisfiable_range_is_the_only_status_a_range_can_raise() {
    let rejection = RangeRejection::NotSatisfiable {
        complete_length: 47_022,
    };

    assert_eq!(rejection.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    declares(&[rejection.status()], RangeRejection::statuses());
    assert_eq!(
        RangeRejection::statuses(),
        [StatusCode::RANGE_NOT_SATISFIABLE]
    );
}

/// The 416 names the representation's length in `Content-Range`.
#[test]
fn only_an_unsatisfiable_range_declares_a_complete_length() {
    use crate::{http::header, response::IntoResponse};

    let response = RangeRejection::NotSatisfiable {
        complete_length: 47_022,
    }
    .into_response();

    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_RANGE)
            .expect("a 416 states the complete length")
            .to_str()
            .expect("a printable field"),
        // Section 15.5.17's own worked example.
        "bytes */47022"
    );

    // No other rejection sets it: a `Content-Range` has no meaning on a status
    // that does not describe its semantics, which is every other one here.
    for other in [
        PathRejection::Invalid {
            name: "id".into(),
            detail: "not a number".into(),
        }
        .into_response(),
        NegotiationRejection::NotAcceptable.into_response(),
        AuthRejection::forbidden().into_response(),
    ] {
        assert!(!other.headers().contains_key(header::CONTENT_RANGE));
    }
}

/// The 416 it declares carries the field it sends.
///
/// The rejection describes its own header where `AuthRejection` does not,
/// because the shape is fixed: there is one `unsatisfied-range` grammar and no
/// per-operation string for a `Describe` to fill in. That is what lets the 416
/// travel with the return type and be declared only where one can arise.
#[test]
fn the_declared_416_carries_the_field_it_sends() {
    use crate::{response::Responses, schema::registry::Registry};

    let declared = RangeRejection::responses(&mut Registry::default());
    let kynos_openapi::RefOr::Item(response) =
        declared.responses.get("416").expect("the only status")
    else {
        panic!("described as a `$ref`");
    };

    assert_eq!(
        declared.responses.keys().collect::<Vec<_>>(),
        ["416"],
        "a range declares one status and no more"
    );

    let kynos_openapi::RefOr::Item(header) = response
        .headers
        .get("Content-Range")
        .expect("RFC 9110 section 15.5.17 asks a 416 to state the complete length")
    else {
        panic!("the header is described as a `$ref`");
    };
    assert_eq!(header.required, Some(true));

    // The 401 is the contrast: only the scheme knows the challenge, so `Auth`
    // declares that header and the rejection cannot.
    let challenged = AuthRejection::responses(&mut Registry::default());
    let kynos_openapi::RefOr::Item(unauthorized) =
        challenged.responses.get("401").expect("the 401")
    else {
        panic!("described as a `$ref`");
    };
    assert!(unauthorized.headers.is_empty());
}

#[test]
fn authentication_and_authorization_are_different_statuses() {
    let observed = [AuthRejection::unauthenticated(), AuthRejection::forbidden()];
    let statuses: Vec<_> = observed.iter().map(AuthRejection::status).collect();

    assert_eq!(statuses, [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN]);
    declares(&statuses, AuthRejection::statuses());
}

/// The whole point of the split: no parameter extractor may reach a status that
/// belongs to authentication or to the body.
#[test]
fn a_parameter_rejection_declares_nothing_but_a_bad_request() {
    for declared in [
        PathRejection::statuses(),
        QueryRejection::statuses(),
        HeaderRejection::statuses(),
    ] {
        assert_eq!(declared, [StatusCode::BAD_REQUEST]);
    }
}

/// 401 and 403 exist in exactly one rejection, so an operation acquires them
/// only through an argument that can raise them.
#[test]
fn only_authentication_declares_a_challenge() {
    for declared in [
        PathRejection::statuses(),
        QueryRejection::statuses(),
        HeaderRejection::statuses(),
        BodyRejection::statuses(),
        NegotiationRejection::statuses(),
        RangeRejection::statuses(),
    ] {
        assert!(!declared.contains(&StatusCode::UNAUTHORIZED));
        assert!(!declared.contains(&StatusCode::FORBIDDEN));
    }
}

#[cfg(feature = "cookie")]
#[test]
fn a_cookie_rejection_is_a_bad_request() {
    use super::CookieRejection;

    let rejection = CookieRejection::Invalid {
        name: "session".into(),
        detail: "not base64".into(),
    };

    assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
    assert_eq!(CookieRejection::statuses(), [StatusCode::BAD_REQUEST]);
}

/// A 403 an authorizer named carries that type on the wire; an unnamed one is
/// still `about:blank`.
///
/// Which authorization rule refused is the application's to say, and only the
/// application knows it — so the URI is a value the rejection carries rather
/// than something Kynos could name for it.
///
/// The rejection is built in a `const` item on purpose. `forbidden_as` takes a
/// `&'static str` and is a `const fn`, which is what makes a named refusal a
/// constant an application declares once rather than a string it formats per
/// request — the compiler, not prose, is what keeps a caller's identifier out
/// of a URI that names a *class* of refusal. This item stops compiling the day
/// either property is dropped.
#[test]
fn an_authorizer_names_the_problem_type_of_its_own_403() {
    const BANNED: &str = "https://example.test/problems/account-banned";
    const REFUSED: AuthRejection = AuthRejection::forbidden_as(BANNED);

    let named = REFUSED.into_problem();

    assert_eq!(named.type_uri, BANNED);
    // The title stays the canonical reason: RFC 9457 section 3.1.3 makes it a
    // property of the type, and an application wanting its own has `ApiError`.
    assert_eq!(named.title, "Forbidden");
    assert_eq!(named.status, StatusCode::FORBIDDEN);
    assert_eq!(named.detail.as_deref(), Some("access is not permitted"));

    // An unnamed 403 is what a 403 has always been, to the byte.
    let unnamed = AuthRejection::forbidden().into_problem();

    assert_eq!(unnamed.type_uri, "about:blank");
    assert_eq!(unnamed.title, named.title);
    assert_eq!(unnamed.status, named.status);
    assert_eq!(unnamed.detail, named.detail);
}

/// Naming a 403 neither survives into a challenge nor is lost to one.
///
/// `with_challenge` rebuilds the rejection, which is the one place a field
/// added to `Forbidden` would be dropped without a word. The 401 is the
/// contrast: it has no field to carry a type, deliberately — saying which
/// credential check refused tells an attacker something a client cannot act
/// on.
#[test]
fn a_named_403_survives_the_challenge_pass_and_still_sends_none() {
    use crate::{http::header, response::IntoResponse};

    const BANNED: &str = "https://example.test/problems/account-banned";
    const CHALLENGE: &str = "Bearer realm=\"api\"";

    let challenged = AuthRejection::forbidden_as(BANNED).with_challenge(Some(CHALLENGE));

    assert_eq!(challenged.challenge(), None);
    assert_eq!(challenged.into_problem().type_uri, BANNED);

    let response = AuthRejection::forbidden_as(BANNED)
        .with_challenge(Some(CHALLENGE))
        .into_response();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));

    let unauthenticated = AuthRejection::unauthenticated()
        .with_challenge(Some(CHALLENGE))
        .into_problem();

    assert_eq!(unauthenticated.type_uri, "about:blank");
}

// --- The closed set --------------------------------------------------------

/// Every rejection type, counted against the ones this file exercises.
///
/// The cases above witness a set someone chose, and nothing tied that set to
/// the rejections the module declares. A rejection added without a case is one
/// whose status nothing checks against what it declares — which is the failure
/// `declares` exists to catch, silently unrun.
///
/// Under `cookie`, because `CookieRejection` is gated there and the full set
/// only exists in that build.
///
/// It counts the `pub enum`s, which is what a *failure* is here: a variant, a
/// status and a sentence. `ScopedRejection<R>` is a struct wrapping one of
/// them and declares no variant, no status and no sentence of its own, so it
/// has nothing this sweep or the ledger below could hold it to — what it adds
/// is a description, and
/// `a_scope_set_that_names_its_refusal_narrows_the_403_to_a_choice` is where
/// that is held.
#[cfg(feature = "cookie")]
#[test]
fn every_rejection_a_caller_can_receive_has_a_case() {
    const SOURCE: &str = include_str!("../rejection.rs");

    /// Every rejection witnessed above, transcribed in declaration order.
    const WITNESSED: [&str; 8] = [
        "PathRejection",
        "QueryRejection",
        "HeaderRejection",
        "CookieRejection",
        "BodyRejection",
        "NegotiationRejection",
        "RangeRejection",
        "AuthRejection",
    ];

    let declared: Vec<&str> = SOURCE
        .lines()
        .filter_map(|line| line.strip_prefix("pub enum "))
        .filter_map(|rest| rest.strip_suffix(" {"))
        .collect();

    assert_eq!(
        declared, WITNESSED,
        "a rejection was added or renamed without a case here"
    );
}

/// One variant of one rejection: its name, the status the ledger expects of
/// it, the status it produces, the set its own type declares, and the sentence
/// it renders.
struct Row {
    name: &'static str,
    expected: StatusCode,
    produced: StatusCode,
    declared: &'static [StatusCode],
    sentence: String,
}

impl Row {
    fn new(
        name: &'static str,
        expected: StatusCode,
        (produced, declared): (StatusCode, &'static [StatusCode]),
        rejection: &impl std::fmt::Display,
    ) -> Self {
        Self {
            name,
            expected,
            produced,
            declared,
            sentence: rejection.to_string(),
        }
    }
}

// One witness per rejection type, each naming its variants in an exhaustive
// match, so a variant added to any of the eight stops this file compiling until
// it is given a row — the idiom `error/tests.rs` uses for `Error`, applied to
// the types a *caller* meets rather than the one a builder does.
// `CookieRejection`'s is under `cookie`, because the type exists only there.

fn path(expected: StatusCode, rejection: &PathRejection) -> Row {
    let name = match rejection {
        PathRejection::Invalid { .. } => "PathRejection::Invalid",
    };
    let statuses = (rejection.status(), PathRejection::statuses());
    Row::new(name, expected, statuses, rejection)
}

fn query(expected: StatusCode, rejection: &QueryRejection) -> Row {
    let name = match rejection {
        QueryRejection::Invalid { .. } => "QueryRejection::Invalid",
        QueryRejection::Schema { .. } => "QueryRejection::Schema",
    };
    let statuses = (rejection.status(), QueryRejection::statuses());
    Row::new(name, expected, statuses, rejection)
}

fn header(expected: StatusCode, rejection: &HeaderRejection) -> Row {
    let name = match rejection {
        HeaderRejection::Invalid { .. } => "HeaderRejection::Invalid",
    };
    let statuses = (rejection.status(), HeaderRejection::statuses());
    Row::new(name, expected, statuses, rejection)
}

#[cfg(feature = "cookie")]
fn cookie(expected: StatusCode, rejection: &super::CookieRejection) -> Row {
    use super::CookieRejection;

    let name = match rejection {
        CookieRejection::Invalid { .. } => "CookieRejection::Invalid",
    };
    let statuses = (rejection.status(), CookieRejection::statuses());
    Row::new(name, expected, statuses, rejection)
}

fn body(expected: StatusCode, rejection: &BodyRejection) -> Row {
    let name = match rejection {
        BodyRejection::Syntax { .. } => "BodyRejection::Syntax",
        BodyRejection::Schema { .. } => "BodyRejection::Schema",
        BodyRejection::UnsupportedMediaType { .. } => "BodyRejection::UnsupportedMediaType",
        BodyRejection::TooLarge { .. } => "BodyRejection::TooLarge",
    };
    let statuses = (rejection.status(), BodyRejection::statuses());
    Row::new(name, expected, statuses, rejection)
}

fn negotiation(expected: StatusCode, rejection: &NegotiationRejection) -> Row {
    let name = match rejection {
        NegotiationRejection::MalformedAccept { .. } => "NegotiationRejection::MalformedAccept",
        NegotiationRejection::NotAcceptable => "NegotiationRejection::NotAcceptable",
    };
    let statuses = (rejection.status(), NegotiationRejection::statuses());
    Row::new(name, expected, statuses, rejection)
}

fn range(expected: StatusCode, rejection: RangeRejection) -> Row {
    let name = match rejection {
        RangeRejection::NotSatisfiable { .. } => "RangeRejection::NotSatisfiable",
    };
    let statuses = (rejection.status(), RangeRejection::statuses());
    Row::new(name, expected, statuses, &rejection)
}

fn auth(expected: StatusCode, rejection: &AuthRejection) -> Row {
    let name = match rejection {
        AuthRejection::Unauthenticated { .. } => "AuthRejection::Unauthenticated",
        AuthRejection::Forbidden { .. } => "AuthRejection::Forbidden",
    };
    let statuses = (rejection.status(), AuthRejection::statuses());
    Row::new(name, expected, statuses, rejection)
}

/// Every variant of every rejection, each with the exact status it must
/// produce.
fn ledger() -> Vec<Row> {
    let text = |detail: &str| detail.to_owned();

    [
        path(
            StatusCode::BAD_REQUEST,
            &PathRejection::Invalid {
                name: text("id"),
                detail: text("not a number"),
            },
        ),
        query(
            StatusCode::BAD_REQUEST,
            &QueryRejection::Invalid {
                name: text("limit"),
                detail: text("not a number"),
            },
        ),
        query(
            StatusCode::BAD_REQUEST,
            &QueryRejection::Schema {
                name: text("querystring"),
                failures: [(text("/limit"), text("must be at most 100"))]
                    .into_iter()
                    .collect(),
            },
        ),
        header(
            StatusCode::BAD_REQUEST,
            &HeaderRejection::Invalid {
                name: text("if-none-match"),
                detail: text("not an entity tag"),
            },
        ),
        #[cfg(feature = "cookie")]
        cookie(
            StatusCode::BAD_REQUEST,
            &super::CookieRejection::Invalid {
                name: text("session"),
                detail: text("not base64"),
            },
        ),
        body(
            StatusCode::BAD_REQUEST,
            &BodyRejection::Syntax {
                detail: text("unexpected end of input"),
            },
        ),
        body(
            StatusCode::UNPROCESSABLE_ENTITY,
            &BodyRejection::Schema {
                failures: [(text("/name"), text("expected a string"))]
                    .into_iter()
                    .collect(),
            },
        ),
        body(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            &BodyRejection::UnsupportedMediaType {
                received: Some(text("text/plain")),
            },
        ),
        body(
            StatusCode::PAYLOAD_TOO_LARGE,
            &BodyRejection::TooLarge { limit: 1_024 },
        ),
        negotiation(
            StatusCode::BAD_REQUEST,
            &NegotiationRejection::MalformedAccept {
                detail: text("a bare comma"),
            },
        ),
        negotiation(
            StatusCode::NOT_ACCEPTABLE,
            &NegotiationRejection::NotAcceptable,
        ),
        range(
            StatusCode::RANGE_NOT_SATISFIABLE,
            RangeRejection::NotSatisfiable {
                complete_length: 1234,
            },
        ),
        auth(StatusCode::UNAUTHORIZED, &AuthRejection::unauthenticated()),
        auth(StatusCode::FORBIDDEN, &AuthRejection::forbidden()),
    ]
    .into()
}

/// Every variant produces exactly the status the ledger expects of it, and its
/// own type declares that status.
///
/// The exhaustive matches above catch a variant added without a *name*; they
/// cannot catch one added without a constructed value, because a match arm
/// nothing reaches still compiles. So the list is transcribed and counted, and
/// each row is checked against its expected status and the set its type
/// advertises.
#[test]
fn every_variant_produces_a_status_its_type_declares() {
    let witnessed = [
        "PathRejection::Invalid",
        "QueryRejection::Invalid",
        "QueryRejection::Schema",
        "HeaderRejection::Invalid",
        #[cfg(feature = "cookie")]
        "CookieRejection::Invalid",
        "BodyRejection::Syntax",
        "BodyRejection::Schema",
        "BodyRejection::UnsupportedMediaType",
        "BodyRejection::TooLarge",
        "NegotiationRejection::MalformedAccept",
        "NegotiationRejection::NotAcceptable",
        "RangeRejection::NotSatisfiable",
        "AuthRejection::Unauthenticated",
        "AuthRejection::Forbidden",
    ];

    let rows = ledger();
    let named: Vec<&str> = rows.iter().map(|row| row.name).collect();

    assert_eq!(named, witnessed, "a variant was added or renamed");

    for Row {
        name,
        expected,
        produced,
        declared,
        ..
    } in rows
    {
        assert_eq!(produced, expected, "{name} produces the wrong status");
        assert!(
            declared.contains(&produced),
            "{name} produces {produced} and its type declares {declared:?}"
        );
    }
}

/// Every variant renders a sentence rather than a debug dump, which is what a
/// caller reporting one prints: no struct syntax, and lowercase with no
/// trailing period, as a Rust error message is written.
#[test]
fn every_variant_renders_a_sentence() {
    for Row { name, sentence, .. } in ledger() {
        assert!(!sentence.is_empty(), "{name} renders nothing");
        assert!(
            !sentence.contains('{'),
            "{name}'s `{sentence}` reads like a debug dump rather than a sentence"
        );
        assert!(
            !sentence.starts_with(char::is_uppercase) && !sentence.ends_with('.'),
            "{name}'s `{sentence}` is not a lowercase, unpunctuated error message"
        );
    }
}

/// The type URI each status a rejection declares narrows `type` to, or `None`
/// where it narrows nothing and refers to the shared component alone.
///
/// Keyed by status and driven from `statuses()`, so a status added to a
/// rejection is described here rather than silently skipped.
fn declared_types<T>() -> std::collections::BTreeMap<u16, Option<String>>
where
    T: IntoProblem + crate::response::Responses,
{
    let mut registry = crate::schema::registry::Registry::new();
    let responses =
        serde_json::to_value(T::responses(&mut registry)).expect("a set of responses serializes");

    T::statuses()
        .iter()
        .map(|status| {
            let schema = &responses[status.as_u16().to_string()]["content"]
                ["application/problem+json"]["schema"];

            (
                status.as_u16(),
                schema["allOf"][1]["properties"]["type"]["const"]
                    .as_str()
                    .map(ToOwned::to_owned),
            )
        })
        .collect()
}

/// Every rejection publishes `about:blank`, so every status it declares says
/// so.
///
/// A response referring to the shared `Problem` component alone would be
/// weaker prose and worse than that on a status a handler's error type also
/// names: a bare `$ref` inside a `oneOf` matches every problem document, so it
/// cannot be one branch of a choice and wins the whole entry instead. Asserted
/// over every rejection rather than one, because the property is
/// `Problem::new`'s and each type reaches it separately.
#[test]
fn every_rejection_declares_the_type_its_problems_carry() {
    let declared = [
        ("PathRejection", declared_types::<PathRejection>()),
        ("QueryRejection", declared_types::<QueryRejection>()),
        ("HeaderRejection", declared_types::<HeaderRejection>()),
        ("BodyRejection", declared_types::<BodyRejection>()),
        (
            "NegotiationRejection",
            declared_types::<NegotiationRejection>(),
        ),
        ("RangeRejection", declared_types::<RangeRejection>()),
        #[cfg(feature = "cookie")]
        (
            "CookieRejection",
            declared_types::<super::CookieRejection>(),
        ),
    ];

    for (rejection, statuses) in declared {
        assert!(!statuses.is_empty(), "{rejection} declares no status");

        for (status, published) in statuses {
            assert_eq!(
                published.as_deref(),
                Some("about:blank"),
                "{rejection}'s {status} does not declare the type it sends"
            );
        }
    }
}

/// The exception, and the reason it is one.
///
/// `AuthRejection::forbidden_as` lets an authorizer put its own URI on a 403,
/// and that value arrives at run time while a description is built from types.
/// A 403 narrowed to `about:blank` would be a claim a named refusal breaks, so
/// it stays the shared component, which admits both. The 401 beside it has no
/// such field and narrows like the rest.
///
/// This is the rejection *type* speaking for itself, which is all it can do: it
/// carries no scope set, so it has no URI to name. `ScopedRejection<R>` below
/// is the one that does, and it narrows.
#[test]
fn the_403_an_authorizer_may_name_is_the_one_status_left_wide() {
    let declared = declared_types::<AuthRejection>();

    assert_eq!(
        declared[&StatusCode::UNAUTHORIZED.as_u16()].as_deref(),
        Some("about:blank")
    );
    assert_eq!(declared[&StatusCode::FORBIDDEN.as_u16()], None);

    // Wide rather than absent, which the assertion above cannot tell apart: the
    // 403 is declared, and what it declares is the component every problem
    // document satisfies.
    let mut registry = crate::schema::registry::Registry::new();
    let responses = serde_json::to_value(<AuthRejection as crate::response::Responses>::responses(
        &mut registry,
    ))
    .expect("a set of responses serializes");

    assert_eq!(
        responses["403"]["content"]["application/problem+json"]["schema"]["$ref"],
        serde_json::json!("#/components/schemas/Problem")
    );
}

/// And the scope set that closes it, at the one seam a type can reach.
///
/// `ScopedRejection<R>` is the same two failures read against `R`, so the 403 a
/// scope set named narrows to a choice between that URI and `about:blank` while
/// the 401 beside it is untouched. Asserted here rather than only over an
/// emitted document because this is the contributor that *decides* the status:
/// a `Responses` returning the wide entry wins the union outright, so a
/// regression here would silently un-narrow every scoped operation while every
/// document still validated.
#[test]
fn a_scope_set_that_names_its_refusal_narrows_the_403_to_a_choice() {
    /// Names one.
    struct Named;

    impl crate::security::auth::Scopes for Named {
        const SCOPES: &'static [&'static str] = &["reports:read"];
        const FORBIDDEN_TYPE: Option<&'static str> = Some("https://errors.example.com/no-scope");
    }

    /// Names none, which is every scope set written before the const existed.
    struct Unnamed;

    impl crate::security::auth::Scopes for Unnamed {
        const SCOPES: &'static [&'static str] = &["reports:read"];
    }

    let mut registry = crate::schema::registry::Registry::new();
    let named = serde_json::to_value(
        <super::ScopedRejection<Named> as crate::response::Responses>::responses(&mut registry),
    )
    .expect("a set of responses serializes");

    let published: Vec<&str> =
        named["403"]["content"]["application/problem+json"]["schema"]["oneOf"]
            .as_array()
            .expect("a choice of two")
            .iter()
            .map(|branch| {
                branch["allOf"][1]["properties"]["type"]["const"]
                    .as_str()
                    .expect("a branch constrains `type` to a const")
            })
            .collect();

    assert_eq!(
        published,
        ["about:blank", "https://errors.example.com/no-scope"],
        "{named}"
    );

    // The 401 is not touched by any of this: `unauthenticated` has no URI field
    // and is not getting one.
    assert_eq!(
        named["401"]["content"]["application/problem+json"]["schema"]["allOf"][1]["properties"]["type"]
            ["const"],
        serde_json::json!("about:blank")
    );

    // Naming none declares what `AuthRejection` declares, which is what every
    // guard declared before a scope set could name anything.
    let unnamed = serde_json::to_value(
        <super::ScopedRejection<Unnamed> as crate::response::Responses>::responses(&mut registry),
    )
    .expect("a set of responses serializes");

    assert_eq!(
        unnamed["403"]["content"]["application/problem+json"]["schema"]["$ref"],
        serde_json::json!("#/components/schemas/Problem")
    );
}
