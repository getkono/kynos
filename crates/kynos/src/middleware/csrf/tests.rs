use super::{Csrf, authority_of, is_safe};
use crate::http::{HeaderMap, HeaderValue, Method};

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

/// One decision the scheme makes: a description, a request, and the verdict.
type Case = (
    &'static str,
    Method,
    &'static [(&'static str, &'static str)],
    bool,
);

/// The cases where `Sec-Fetch-Site` decides, or the method does.
fn browser_cases() -> Vec<Case> {
    vec![
        // Safe methods, whatever they claim.
        (
            "a GET from another site",
            Method::GET,
            &[("sec-fetch-site", "cross-site")],
            true,
        ),
        (
            "a HEAD from another site",
            Method::HEAD,
            &[("sec-fetch-site", "cross-site")],
            true,
        ),
        // A preflight carries no credentials and reaches no operation.
        (
            "an OPTIONS preflight",
            Method::OPTIONS,
            &[("sec-fetch-site", "cross-site")],
            true,
        ),
        // The browser's own statement, which script cannot forge.
        (
            "same-origin",
            Method::POST,
            &[("sec-fetch-site", "same-origin")],
            true,
        ),
        (
            "none, so no page caused it",
            Method::POST,
            &[("sec-fetch-site", "none")],
            true,
        ),
        (
            "cross-site",
            Method::POST,
            &[("sec-fetch-site", "cross-site")],
            false,
        ),
        (
            "same-site, which is not same-origin",
            Method::POST,
            &[("sec-fetch-site", "same-site")],
            false,
        ),
        // `Sec-Fetch-Site` wins over `Origin`: it is the field that cannot lie.
        (
            "cross-site claiming a trusted origin",
            Method::POST,
            &[
                ("sec-fetch-site", "cross-site"),
                ("origin", "https://admin.example.com"),
            ],
            false,
        ),
        // An older browser sends `Origin` and no `Sec-Fetch-Site`.
    ]
}

/// The cases where no `Sec-Fetch-Site` arrived and `Origin` decides.
fn legacy_cases() -> Vec<Case> {
    vec![
        (
            "a trusted origin, no fetch metadata",
            Method::POST,
            &[("origin", "https://admin.example.com")],
            true,
        ),
        (
            "an untrusted origin, no fetch metadata",
            Method::POST,
            &[
                ("origin", "https://evil.example.com"),
                ("host", "api.example.com"),
            ],
            false,
        ),
        (
            "an origin matching the request's own host",
            Method::POST,
            &[
                ("origin", "https://api.example.com"),
                ("host", "api.example.com"),
            ],
            true,
        ),
        (
            "an origin whose host differs only in case",
            Method::POST,
            &[
                ("origin", "https://API.example.com"),
                ("host", "api.example.com"),
            ],
            true,
        ),
        (
            "an origin with no matching host field",
            Method::POST,
            &[("origin", "https://api.example.com")],
            false,
        ),
        // Neither field: not a browser, so not subject to CSRF.
        ("no fetch metadata and no origin", Method::POST, &[], true),
        (
            "a DELETE from nothing browser-like",
            Method::DELETE,
            &[("host", "api.example.com")],
            true,
        ),
    ]
}

/// Every case the scheme decides, and what it decides.
///
/// One table rather than a case apiece: the rules are an ordered list where the
/// first match wins, and a table is the only shape that shows an earlier rule
/// shadowing a later one.
#[test]
fn every_rule_the_scheme_states_is_applied_in_order() {
    let csrf = Csrf::new().trusting_origin("https://admin.example.com");

    for (description, method, fields, expected) in browser_cases().into_iter().chain(legacy_cases())
    {
        assert_eq!(
            csrf.permits(&method, &map(fields)),
            expected,
            "{description}"
        );
    }
}

/// Trusting one origin does not trust its subdomains.
///
/// A CSRF allow-list that admits a subdomain admits whoever takes that
/// subdomain over, which is a common enough way in to be worth a case.
#[test]
fn a_trusted_origin_does_not_trust_anything_beneath_it() {
    let csrf = Csrf::new().trusting_origin("https://example.com");

    assert!(!csrf.permits(
        &Method::POST,
        &map(&[
            ("origin", "https://evil.example.com"),
            ("host", "api.example.com")
        ])
    ));
}

/// Every method RFC 9110 section 9.2.1 calls safe, and the ones it does not.
#[test]
fn the_safe_methods_are_the_ones_the_specification_names() {
    for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
        assert!(is_safe(&method), "{method} is safe");
    }
    for method in [
        Method::POST,
        Method::PUT,
        Method::PATCH,
        Method::DELETE,
        Method::TRACE,
    ] {
        assert!(
            !is_safe(&method),
            "{method} is not read-only for this purpose"
        );
    }
}

/// An origin's authority is what is compared against `Host`.
#[test]
fn an_origin_reduces_to_its_authority() {
    assert_eq!(authority_of("https://api.example.com"), "api.example.com");
    assert_eq!(
        authority_of("http://api.example.com:8080"),
        "api.example.com:8080"
    );
    assert_eq!(authority_of("https://API.Example.COM"), "api.example.com");
    // `null` is an origin in its own right and reduces to itself, so it is
    // never equal to a host and is refused unless explicitly trusted.
    assert_eq!(authority_of("null"), "null");
}
