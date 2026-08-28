use super::{DEFAULT_CORRELATION, REDACTED, Trace};
use crate::{
    extract::params::header::HeaderParams,
    http::{HeaderMap, HeaderValue},
    middleware::request_id::XRequestId,
};

/// A header map from pairs.
fn map(fields: &[(&str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in fields {
        headers.append(
            crate::http::HeaderName::from_bytes(name.as_bytes()).expect("a legal field name"),
            HeaderValue::from_str(value).expect("a printable field"),
        );
    }
    headers
}

/// Every name on the denylist is recorded as present and never by value.
///
/// A sweep of the whole list rather than a case for `authorization`: the
/// list is the guarantee, and a name added to it without being covered here
/// would be a name nothing checks.
#[test]
fn every_redacted_header_is_recorded_without_its_value() {
    for name in REDACTED {
        let trace = Trace::new().record_headers(std::slice::from_ref(name));
        let recorded = trace.recorded(&map(&[(name, "s3cret-value")]));

        assert!(
            recorded.contains("<redacted>"),
            "`{name}` was recorded verbatim: {recorded}"
        );
        assert!(
            !recorded.contains("s3cret-value"),
            "`{name}` leaked its value: {recorded}"
        );
    }
}

/// The case a denylist must not break: an ordinary header still records.
///
/// Without this the test above passes for a `recorded` that redacts
/// everything, which would make the feature useless rather than safe.
#[test]
fn an_ordinary_header_is_recorded_with_its_value() {
    let trace = Trace::new().record_headers(&["x-tenant"]);
    let recorded = trace.recorded(&map(&[("x-tenant", "acme")]));

    assert_eq!(recorded, "x-tenant=acme");
}

/// The denylist is matched case-insensitively, per RFC 9110 section 5.1.
#[test]
fn a_redacted_name_in_another_case_is_still_redacted() {
    let trace = Trace::new().record_headers(&["Authorization"]);
    let recorded = trace.recorded(&map(&[("authorization", "Bearer eyJ")]));

    assert!(recorded.contains("<redacted>"), "{recorded}");
    assert!(!recorded.contains("eyJ"), "{recorded}");
}

/// The correlation name comes from the group, not from a second copy of it.
#[test]
fn the_correlation_name_is_read_from_the_group() {
    assert_eq!(XRequestId::NAMES, [DEFAULT_CORRELATION]);
    assert_eq!(
        Trace::new().correlating::<XRequestId>().correlation,
        DEFAULT_CORRELATION
    );
}
