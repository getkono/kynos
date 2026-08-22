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
    ];

    let statuses: Vec<_> = observed.iter().map(BodyRejection::status).collect();

    assert_eq!(
        statuses,
        [
            StatusCode::BAD_REQUEST,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ]
    );
    declares(&statuses, BodyRejection::statuses());
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
        AuthRejection::Forbidden.into_response(),
    ] {
        assert!(!other.headers().contains_key(header::CONTENT_RANGE));
    }
}

#[test]
fn authentication_and_authorization_are_different_statuses() {
    let observed = [AuthRejection::unauthenticated(), AuthRejection::Forbidden];
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

/// Every variant of every rejection: its name, the status it produces, and the
/// set its own type declares.
///
/// The matches are exhaustive, so a variant added to any of the seven stops
/// this file compiling until it is given a row — the idiom `error/tests.rs`
/// uses for `Error`, applied to the types a *caller* meets rather than the one
/// a builder does.
fn ledger() -> Vec<(&'static str, StatusCode, &'static [StatusCode])> {
    fn path(rejection: PathRejection) -> (&'static str, StatusCode, &'static [StatusCode]) {
        let name = match rejection {
            PathRejection::Invalid { .. } => "PathRejection::Invalid",
        };
        (name, rejection.status(), PathRejection::statuses())
    }

    fn query(rejection: QueryRejection) -> (&'static str, StatusCode, &'static [StatusCode]) {
        let name = match rejection {
            QueryRejection::Invalid { .. } => "QueryRejection::Invalid",
        };
        (name, rejection.status(), QueryRejection::statuses())
    }

    fn header(rejection: HeaderRejection) -> (&'static str, StatusCode, &'static [StatusCode]) {
        let name = match rejection {
            HeaderRejection::Invalid { .. } => "HeaderRejection::Invalid",
        };
        (name, rejection.status(), HeaderRejection::statuses())
    }

    fn body(rejection: BodyRejection) -> (&'static str, StatusCode, &'static [StatusCode]) {
        let name = match rejection {
            BodyRejection::Syntax { .. } => "BodyRejection::Syntax",
            BodyRejection::Schema { .. } => "BodyRejection::Schema",
            BodyRejection::UnsupportedMediaType { .. } => "BodyRejection::UnsupportedMediaType",
        };
        (name, rejection.status(), BodyRejection::statuses())
    }

    fn negotiation(
        rejection: NegotiationRejection,
    ) -> (&'static str, StatusCode, &'static [StatusCode]) {
        let name = match rejection {
            NegotiationRejection::MalformedAccept { .. } => "NegotiationRejection::MalformedAccept",
            NegotiationRejection::NotAcceptable => "NegotiationRejection::NotAcceptable",
        };
        (name, rejection.status(), NegotiationRejection::statuses())
    }

    fn range(rejection: RangeRejection) -> (&'static str, StatusCode, &'static [StatusCode]) {
        let name = match rejection {
            RangeRejection::NotSatisfiable { .. } => "RangeRejection::NotSatisfiable",
        };
        (name, rejection.status(), RangeRejection::statuses())
    }

    fn auth(rejection: AuthRejection) -> (&'static str, StatusCode, &'static [StatusCode]) {
        let name = match rejection {
            AuthRejection::Unauthenticated { .. } => "AuthRejection::Unauthenticated",
            AuthRejection::Forbidden => "AuthRejection::Forbidden",
        };
        (name, rejection.status(), AuthRejection::statuses())
    }

    let text = |detail: &str| detail.to_owned();

    vec![
        path(PathRejection::Invalid {
            name: text("id"),
            detail: text("not a number"),
        }),
        query(QueryRejection::Invalid {
            name: text("limit"),
            detail: text("not a number"),
        }),
        header(HeaderRejection::Invalid {
            name: text("if-none-match"),
            detail: text("not an entity tag"),
        }),
        body(BodyRejection::Syntax {
            detail: text("unexpected end of input"),
        }),
        body(BodyRejection::Schema {
            failures: [(text("/name"), text("expected a string"))]
                .into_iter()
                .collect(),
        }),
        body(BodyRejection::UnsupportedMediaType {
            received: Some(text("text/plain")),
        }),
        negotiation(NegotiationRejection::MalformedAccept {
            detail: text("a bare comma"),
        }),
        negotiation(NegotiationRejection::NotAcceptable),
        range(RangeRejection::NotSatisfiable {
            complete_length: 1234,
        }),
        auth(AuthRejection::unauthenticated()),
        auth(AuthRejection::Forbidden),
    ]
}

/// Every variant produces a status its own type declares.
///
/// The exhaustive matches above catch a variant added without a *name*; they
/// cannot catch one added without a constructed value, because a match arm
/// nothing reaches still compiles. So the list is transcribed and counted, and
/// each row is checked against the set its type advertises.
#[test]
fn every_variant_produces_a_status_its_type_declares() {
    const WITNESSED: [&str; 11] = [
        "PathRejection::Invalid",
        "QueryRejection::Invalid",
        "HeaderRejection::Invalid",
        "BodyRejection::Syntax",
        "BodyRejection::Schema",
        "BodyRejection::UnsupportedMediaType",
        "NegotiationRejection::MalformedAccept",
        "NegotiationRejection::NotAcceptable",
        "RangeRejection::NotSatisfiable",
        "AuthRejection::Unauthenticated",
        "AuthRejection::Forbidden",
    ];

    let rows = ledger();
    let named: Vec<&str> = rows.iter().map(|(name, _, _)| *name).collect();

    assert_eq!(named, WITNESSED, "a variant was added or renamed");

    for (name, produced, declared) in rows {
        assert!(
            declared.contains(&produced),
            "{name} produces {produced} and its type declares {declared:?}"
        );
    }
}

/// Every named variant renders a sentence rather than a debug dump, which is
/// what a caller reporting one prints.
#[test]
fn every_variant_renders_a_sentence() {
    for rejection in [
        BodyRejection::Syntax {
            detail: "unexpected end of input".to_owned(),
        },
        BodyRejection::Schema {
            failures: [("/name".to_owned(), "expected a string".to_owned())]
                .into_iter()
                .collect(),
        },
        BodyRejection::UnsupportedMediaType {
            received: Some("text/plain".to_owned()),
        },
    ] {
        let rendered = rejection.to_string();

        assert!(!rendered.is_empty());
        assert!(
            !rendered.contains('{'),
            "`{rendered}` reads like a debug dump rather than a sentence"
        );
    }
}
