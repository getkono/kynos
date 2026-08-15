use super::{
    AuthRejection, BodyRejection, HeaderRejection, NegotiationRejection, PathRejection,
    QueryRejection,
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

#[test]
fn authentication_and_authorization_are_different_statuses() {
    let observed = [AuthRejection::Unauthenticated, AuthRejection::Forbidden];
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
