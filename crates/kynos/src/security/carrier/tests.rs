use super::{KeyLocation, api_key, basic, bearer, http_scheme};
use crate::http::{HeaderName, HeaderValue, Parts, Request, header::AUTHORIZATION};

/// A request head carrying `value` as its `Authorization`.
fn authorized(value: &str) -> Parts {
    let mut request = Request::new(crate::http::body::Body::empty());
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(value).expect("a printable field"),
    );
    request.into_parts().0
}

/// A request head with no fields at all.
fn bare() -> Parts {
    Request::new(crate::http::body::Body::empty())
        .into_parts()
        .0
}

// --- Bearer, RFC 6750 section 2.1 -----------------------------------------

#[test]
fn a_bearer_token_is_everything_after_the_scheme() {
    let head = authorized("Bearer eyJhbGciOiJIUzI1NiJ9.e30.sig");
    let token = bearer(&head).expect("a credential").expect("present");

    assert_eq!(token.as_str(), "eyJhbGciOiJIUzI1NiJ9.e30.sig");
}

/// RFC 9110 section 11.1 makes the scheme name case-insensitive.
///
/// The failure this rules out is the one every hand-rolled authenticator has:
/// `strip_prefix("Bearer ")` silently ignores a client that wrote `bearer`.
#[test]
fn a_bearer_token_is_read_whatever_case_the_scheme_was_written_in() {
    for spelling in ["Bearer", "bearer", "BEARER", "BeArEr"] {
        let head = authorized(&format!("{spelling} abc"));
        assert_eq!(
            bearer(&head)
                .expect("a credential")
                .expect("present")
                .as_str(),
            "abc",
            "{spelling}"
        );
    }
}

#[test]
fn no_authorization_field_is_absence_rather_than_failure() {
    assert!(bearer(&bare()).expect("absence is not a failure").is_none());
}

/// A credential for another scheme is a refusal, not absence.
///
/// Reporting absence would let `MaybeAuth` wave through a request that
/// presented something nobody checked.
#[test]
fn a_credential_for_another_scheme_is_refused_rather_than_ignored() {
    assert!(bearer(&authorized("Basic dXNlcjpwYXNz")).is_err());
    assert!(basic(&authorized("Bearer abc")).is_err());
}

#[test]
fn a_bearer_scheme_with_no_token_is_not_a_credential() {
    assert!(bearer(&authorized("Bearer ")).is_err());
}

// --- Basic, RFC 7617 -------------------------------------------------------

#[test]
fn basic_credentials_split_at_the_first_colon() {
    // `dXNlcjpwYTpzcw==` is `user:pa:ss`.
    let head = authorized("Basic dXNlcjpwYTpzcw==");
    let credentials = basic(&head).expect("a credential").expect("present");

    assert_eq!(credentials.username(), "user");
    assert_eq!(
        credentials.password(),
        "pa:ss",
        "a password may hold a colon; a user-id may not"
    );
}

#[test]
fn an_empty_password_is_a_password() {
    // `dXNlcjo=` is `user:`.
    let credentials = basic(&authorized("Basic dXNlcjo="))
        .expect("a credential")
        .expect("present");

    assert_eq!(credentials.into_parts(), ("user".to_owned(), String::new()));
}

/// One case per way `basic` refuses what is not a basic credential.
#[test]
fn every_malformed_basic_credential_is_refused() {
    let cases = [
        ("base64 that does not decode", "Basic !!!!"),
        // `/w==` is the byte 0xff, which is not UTF-8.
        ("bytes that are not UTF-8", "Basic /w=="),
        // `dXNlcm5hbWU=` is `username`, with no colon.
        ("no colon at all", "Basic dXNlcm5hbWU="),
    ];

    for (description, field) in cases {
        assert!(
            basic(&authorized(field)).is_err(),
            "{description}: {field:?}"
        );
    }
}

// --- An arbitrary HTTP scheme ---------------------------------------------

#[test]
fn an_arbitrary_scheme_yields_both_halves() {
    let head = authorized("Negotiate YIIB...");
    let read = http_scheme(&head, "negotiate")
        .expect("a credential")
        .expect("present");

    assert_eq!(read.scheme(), "Negotiate", "the spelling the client used");
    assert_eq!(read.credentials(), "YIIB...");
}

// --- API keys --------------------------------------------------------------

#[test]
fn a_header_api_key_is_read_from_the_field_the_scheme_named() {
    let mut request = Request::new(crate::http::body::Body::empty());
    request.headers_mut().insert(
        HeaderName::from_static("x-api-key"),
        HeaderValue::from_static("k-123"),
    );
    let head = request.into_parts().0;

    let key = api_key(&head, KeyLocation::Header, "x-api-key")
        .expect("a credential")
        .expect("present");
    assert_eq!(key.as_str(), "k-123");
}

#[test]
fn a_query_api_key_is_percent_decoded() {
    let mut request = Request::new(crate::http::body::Body::empty());
    *request.uri_mut() = "/reports?api_key=k%20123&other=1"
        .parse()
        .expect("a legal target");
    let head = request.into_parts().0;

    let key = api_key(&head, KeyLocation::Query, "api_key")
        .expect("a credential")
        .expect("present");
    assert_eq!(
        key.as_str(),
        "k 123",
        "a key is a value, so its encoding is not part of it"
    );
}

/// A request head whose target is `target`.
fn targeting(target: &str) -> Parts {
    let mut request = Request::new(crate::http::body::Body::empty());
    *request.uri_mut() = target.parse().expect("a legal target");
    request.into_parts().0
}

/// An API key `in: query` is a query parameter, so it is read with form rules,
/// as `references/3.1.2.md` requires of every `in: query` parameter: an
/// unescaped `+` is a space, and a literal plus arrives as `%2B`.
#[test]
fn a_query_api_key_reads_plus_as_a_space() {
    for (raw, expected) in [("a+b", "a b"), ("a%2Bb", "a+b"), ("a%20b", "a b")] {
        let head = targeting(&format!("/reports?api_key={raw}"));
        let key = api_key(&head, KeyLocation::Query, "api_key")
            .expect("a credential")
            .expect("present");
        assert_eq!(key.as_str(), expected, "{raw}");
    }
}

/// The first pair named the key decides, as it does for a derived query
/// parameter; a later pair never stands in for one that could not be read.
#[test]
fn a_query_api_key_is_the_first_pair_named_the_key() {
    let head = targeting("/reports?api_key&api_key=second");
    let key = api_key(&head, KeyLocation::Query, "api_key")
        .expect("a credential")
        .expect("present");
    assert_eq!(key.as_str(), "", "a pair with no `=` has an empty value");

    let head = targeting("/reports?api_key=%FF&api_key=second");
    assert!(
        api_key(&head, KeyLocation::Query, "api_key").is_err(),
        "a first value that is not UTF-8 is present and malformed"
    );
}

#[test]
fn a_query_api_key_the_target_does_not_carry_is_absent() {
    let mut request = Request::new(crate::http::body::Body::empty());
    *request.uri_mut() = "/reports?other=1".parse().expect("a legal target");
    let head = request.into_parts().0;

    assert!(
        api_key(&head, KeyLocation::Query, "api_key")
            .expect("absence is not a failure")
            .is_none()
    );
}

#[test]
fn a_cookie_api_key_reads_the_same_jar_a_parameter_does() {
    let mut request = Request::new(crate::http::body::Body::empty());
    request.headers_mut().append(
        crate::http::header::COOKIE,
        HeaderValue::from_static("other=1; session=s-42"),
    );
    let head = request.into_parts().0;

    let key = api_key(&head, KeyLocation::Cookie, "session")
        .expect("a credential")
        .expect("present");
    assert_eq!(key.as_str(), "s-42");
}

/// A request head carrying each of `fields` as a `Cookie` field, in order.
fn with_cookies(fields: &[&[u8]]) -> Parts {
    let mut request = Request::new(crate::http::body::Body::empty());
    for field in fields {
        request.headers_mut().append(
            crate::http::header::COOKIE,
            HeaderValue::from_bytes(field).expect("a legal field value"),
        );
    }
    request.into_parts().0
}

/// The first cookie of the key's name decides, as the first query pair does: a
/// key that was sent and cannot be read is refused, never read as absent and
/// never stood in for by a later cookie of the same name.
#[test]
fn a_cookie_api_key_that_is_not_ascii_is_refused() {
    for fields in [
        &[b"session=s-\xff".as_slice()][..],
        &[b"session=s-\xff; session=s-42"],
        &[b"session=s-\xff", b"session=s-42"],
    ] {
        let shown: Vec<_> = fields
            .iter()
            .map(|field| field.escape_ascii().to_string())
            .collect();
        assert!(
            api_key(&with_cookies(fields), KeyLocation::Cookie, "session").is_err(),
            "{shown:?}"
        );
    }
}

/// An unreadable cookie of another name hides only itself, even on the line
/// that carries the key.
#[test]
fn an_unreadable_cookie_of_another_name_does_not_hide_the_key() {
    let head = with_cookies(&[b"other=\xff; session=s-42"]);

    let key = api_key(&head, KeyLocation::Cookie, "session")
        .expect("a credential")
        .expect("present");
    assert_eq!(key.as_str(), "s-42");
}

/// Every location an API key may travel in, and the case that covers it.
///
/// The mapping is an exhaustive match rather than a count of source text, so a
/// fourth location stops this file compiling until it is witnessed — which is
/// the stronger of the two guards, and the one `wire_tag` in
/// `kynos-openapi`'s security tests already uses.
#[test]
fn every_key_location_has_a_case() {
    let all = [KeyLocation::Header, KeyLocation::Query, KeyLocation::Cookie];

    for location in all {
        let covered_by = match location {
            KeyLocation::Header => "a_header_api_key_is_read_from_the_field_the_scheme_named",
            KeyLocation::Query => "a_query_api_key_is_percent_decoded",
            KeyLocation::Cookie => "a_cookie_api_key_reads_the_same_jar_a_parameter_does",
        };
        assert!(!covered_by.is_empty(), "{location:?} names no case");
    }

    assert_eq!(
        all.len(),
        3,
        "the specification permits an API key in a header, a query parameter or a cookie"
    );
}

#[test]
fn an_owned_key_survives_being_taken_from_the_request() {
    let mut request = Request::new(crate::http::body::Body::empty());
    request.headers_mut().insert(
        HeaderName::from_static("x-api-key"),
        HeaderValue::from_static("k-123"),
    );
    let head = request.into_parts().0;

    let key = api_key(&head, KeyLocation::Header, "x-api-key")
        .expect("a credential")
        .expect("present");
    assert_eq!(key.into_inner(), "k-123");
}

// --- Debug output ----------------------------------------------------------

/// `rendered` names the credential and does not contain `secret`.
fn assert_redacted(rendered: &str, expected: &str, secret: &str) {
    assert!(!rendered.contains(secret), "{rendered} prints the secret");
    assert_eq!(rendered, expected);
}

#[test]
fn a_bearer_token_is_redacted_from_debug_output() {
    let token = bearer(&authorized("Bearer s3cret"))
        .expect("a credential")
        .expect("present");

    assert_redacted(&format!("{token:?}"), "BearerToken(<redacted>)", "s3cret");
}

/// The user-id identifies whom a log line is about; only the password replays.
#[test]
fn basic_credentials_show_the_user_id_and_redact_the_password() {
    let credentials = basic(&authorized("Basic YWxpY2U6czNjcmV0"))
        .expect("a credential")
        .expect("present");

    assert_redacted(
        &format!("{credentials:?}"),
        r#"Credentials { username: "alice", password: <redacted> }"#,
        "s3cret",
    );
}

#[test]
fn scheme_credentials_show_the_scheme_and_redact_the_credentials() {
    let credentials = http_scheme(&authorized("Negotiate s3cret"), "Negotiate")
        .expect("a credential")
        .expect("present");

    assert_redacted(
        &format!("{credentials:?}"),
        r#"SchemeCredentials { scheme: "Negotiate", credentials: <redacted> }"#,
        "s3cret",
    );
}

#[test]
fn an_api_key_is_redacted_from_debug_output() {
    let mut request = Request::new(crate::http::body::Body::empty());
    request.headers_mut().insert(
        HeaderName::from_static("x-api-key"),
        HeaderValue::from_static("s3cret"),
    );
    let head = request.into_parts().0;
    let key = api_key(&head, KeyLocation::Header, "x-api-key")
        .expect("a credential")
        .expect("present");

    assert_redacted(&format!("{key:?}"), "ApiKey(<redacted>)", "s3cret");
}
