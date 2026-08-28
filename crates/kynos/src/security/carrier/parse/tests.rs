use super::{authorization, scheme_is};
use crate::http::{HeaderValue, Request, header::AUTHORIZATION};

/// A request head carrying each of `fields` as an `Authorization`.
fn parts(fields: &[&str]) -> crate::http::Parts {
    let mut request = Request::new(crate::http::body::Body::empty());
    for field in fields {
        request.headers_mut().append(
            AUTHORIZATION,
            HeaderValue::from_str(field).expect("a printable field"),
        );
    }
    request.into_parts().0
}

#[test]
fn a_request_carrying_no_credential_is_anonymous_rather_than_malformed() {
    assert_eq!(
        authorization(&parts(&[])).expect("no field is not a failure"),
        None
    );
}

#[test]
fn a_well_formed_field_splits_at_the_scheme() {
    let head = parts(&["Bearer abc.def"]);
    let read = authorization(&head)
        .expect("a credential")
        .expect("present");

    assert_eq!(read.scheme, "Bearer");
    assert_eq!(read.credentials, "abc.def");
}

/// RFC 9110 section 11.6.2 permits bad whitespace between the two halves.
#[test]
fn extra_space_after_the_scheme_is_not_part_of_the_credential() {
    let head = parts(&["Basic    dXNlcjpwYXNz"]);
    let read = authorization(&head)
        .expect("a credential")
        .expect("present");

    assert_eq!(read.credentials, "dXNlcjpwYXNz");
}

/// One case per way `authorization` refuses, counted against the refusals.
#[test]
fn every_refusal_has_a_case() {
    const SOURCE: &str = include_str!("../parse.rs");
    // Spelled in two pieces: `SOURCE` is this file, and a contiguous
    // literal would count itself.
    const NEEDLE: &str = concat!("AuthRejection", "::unauthenticated");

    let cases: &[(&str, &[&str])] = &[
        // Two fields: choosing either would let a proxy that appended one
        // decide which credential the service honours.
        ("two Authorization fields", &["Bearer a", "Bearer b"]),
        ("bytes no `&str` can hold", &[]),
        ("a value with no scheme token", &["justatoken"]),
        ("an empty scheme", &[" credentials"]),
    ];

    for (description, fields) in cases {
        let head = if fields.is_empty() {
            let mut request = Request::new(crate::http::body::Body::empty());
            request.headers_mut().append(
                AUTHORIZATION,
                HeaderValue::from_bytes(b"Bearer \xff").expect("a legal field value"),
            );
            request.into_parts().0
        } else {
            parts(fields)
        };

        assert!(
            authorization(&head).is_err(),
            "{description} must not read as a credential"
        );
    }

    let refusals = SOURCE.matches(NEEDLE).count();
    assert_eq!(
        refusals,
        cases.len(),
        "`parse.rs` refuses {refusals} way(s) and {} have a case",
        cases.len()
    );
}

/// RFC 9110 section 11.1: a scheme name is case-insensitive.
#[test]
fn a_scheme_is_named_whatever_case_it_is_written_in() {
    for spelling in ["Bearer", "bearer", "BEARER", "BeArEr"] {
        assert!(scheme_is(spelling, "bearer"), "{spelling}");
    }
    assert!(!scheme_is("Basic", "bearer"));
}
