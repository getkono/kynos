//! Reading the `Authorization` field, per RFC 9110 section 11.6.2.

use crate::{
    error::rejection::AuthRejection,
    http::{Parts, header::AUTHORIZATION},
};

/// The two halves of an `Authorization` field value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Authorization<'r> {
    /// The RFC 9110 section 11.1 scheme token, as the client spelled it.
    pub(super) scheme: &'r str,
    /// Everything after the single space that follows the scheme.
    pub(super) credentials: &'r str,
}

/// Reads the request's `Authorization` field.
///
/// `Ok(None)` when there is none, which is anonymity rather than a failure —
/// the caller decides whether that is acceptable. `Err` when there is one and
/// it is not a credential: two fields, bytes no `&str` can hold, or a value
/// with no scheme token.
pub(super) fn authorization(parts: &Parts) -> Result<Option<Authorization<'_>>, AuthRejection> {
    let mut fields = parts.headers.get_all(AUTHORIZATION).into_iter();

    let Some(field) = fields.next() else {
        return Ok(None);
    };

    // RFC 9110 section 5.3 makes `Authorization` a singleton field. Two of them
    // is not a credential to choose between: picking either would mean a proxy
    // that appended one could decide which credential a service honours.
    if fields.next().is_some() {
        return Err(AuthRejection::unauthenticated());
    }

    let value = field
        .to_str()
        .map_err(|_| AuthRejection::unauthenticated())?;

    // `scheme SP credentials`, and the scheme is a token so it holds no space.
    let (scheme, credentials) = value
        .split_once(' ')
        .ok_or_else(AuthRejection::unauthenticated)?;

    if scheme.is_empty() {
        return Err(AuthRejection::unauthenticated());
    }

    Ok(Some(Authorization {
        scheme,
        // RFC 9110 section 11.6.2 permits bad whitespace after the scheme.
        credentials: credentials.trim_start_matches(' '),
    }))
}

/// Whether `presented` names `expected`.
///
/// RFC 9110 section 11.1 makes an authentication scheme name case-insensitive,
/// so `bearer`, `Bearer` and `BEARER` are one scheme. Getting this wrong is not
/// pedantry: a client that spells it in lower case is one whose credential a
/// case-sensitive comparison silently ignores.
pub(super) fn scheme_is(presented: &str, expected: &str) -> bool {
    presented.eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
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
        const SOURCE: &str = include_str!("parse.rs");
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
}
