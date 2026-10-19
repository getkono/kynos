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

/// `permits` over a request that states its authority in `Host` alone.
fn permits(csrf: &Csrf, method: &Method, fields: &[(&str, &str)]) -> bool {
    csrf.permits(method, &map(fields), None)
}

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
        // A trusted `Origin` is tried before `Sec-Fetch-Site`, because every
        // current browser says `cross-site` from a front end served elsewhere,
        // and script can no more set `Origin` than it can `Sec-Fetch-Site`.
        (
            "cross-site from a trusted origin",
            Method::POST,
            &[
                ("sec-fetch-site", "cross-site"),
                ("origin", "https://admin.example.com"),
            ],
            true,
        ),
        (
            "same-site from a trusted origin",
            Method::POST,
            &[
                ("sec-fetch-site", "same-site"),
                ("origin", "https://admin.example.com"),
            ],
            true,
        ),
        (
            "cross-site from an untrusted origin",
            Method::POST,
            &[
                ("sec-fetch-site", "cross-site"),
                ("origin", "https://evil.example.com"),
            ],
            false,
        ),
        // The own-host match is the fallback for a browser without fetch
        // metadata; one that sent it has already said where the request came
        // from, and that statement stands.
        (
            "cross-site with an origin matching the request's own host",
            Method::POST,
            &[
                ("sec-fetch-site", "cross-site"),
                ("origin", "https://api.example.com"),
                ("host", "api.example.com"),
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
            "an origin with no authority to match it against",
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
        assert_eq!(permits(&csrf, &method, fields), expected, "{description}");
    }
}

/// Trusting one origin does not trust its subdomains.
///
/// A CSRF allow-list that admits a subdomain admits whoever takes that
/// subdomain over, which is a common enough way in to be worth a case.
#[test]
fn a_trusted_origin_does_not_trust_anything_beneath_it() {
    let csrf = Csrf::new().trusting_origin("https://example.com");

    assert!(!permits(
        &csrf,
        &Method::POST,
        &[
            ("origin", "https://evil.example.com"),
            ("host", "api.example.com")
        ]
    ));
}

/// A version-2 request states its authority on the target, not in a field.
///
/// RFC 9113 section 8.3.1 replaces `Host` with the `:authority` pseudo-header,
/// which `http` puts on the request URI. Reading `Host` alone therefore found
/// no authority on any HTTP/2 request, so a same-origin `POST` from a browser
/// old enough to send `Origin` without `Sec-Fetch-Site` -- Safari before 16.4,
/// which speaks HTTP/2 -- was refused outright.
#[test]
fn a_request_stating_its_authority_on_the_target_matches_its_own_origin() {
    let csrf = Csrf::new();

    assert!(
        csrf.permits(
            &Method::POST,
            &map(&[("origin", "https://api.example.com")]),
            Some("api.example.com"),
        ),
        "a same-origin request was refused because its authority was not in `Host`"
    );
}

/// The control: the target's authority does not admit another origin.
///
/// Without it the case above passes for a `permits` that stopped comparing.
#[test]
fn a_target_authority_does_not_admit_another_origin() {
    let csrf = Csrf::new();

    assert!(!csrf.permits(
        &Method::POST,
        &map(&[("origin", "https://evil.example.com")]),
        Some("api.example.com"),
    ));
}

/// The target's authority decides where both are present.
///
/// An HTTP/1.1 request carries an authority on its target only in absolute
/// form, and RFC 9112 section 3.2.2 says an origin server receiving one "MUST
/// ignore the received Host header field". A version-2 request's `:authority`
/// is on the target too, and RFC 9113 section 8.3.1 makes it, not `Host`, the
/// field that conveys the authority.
#[test]
fn the_target_authority_decides_where_a_request_states_both() {
    let csrf = Csrf::new();

    assert!(csrf.permits(
        &Method::POST,
        &map(&[
            ("origin", "https://api.example.com"),
            ("host", "other.example.com"),
        ]),
        Some("api.example.com"),
    ));
    assert!(
        !csrf.permits(
            &Method::POST,
            &map(&[
                ("origin", "https://other.example.com"),
                ("host", "other.example.com"),
            ]),
            Some("api.example.com"),
        ),
        "an origin matching only the ignored `Host` was admitted"
    );
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

/// A trusted origin is compared ASCII case-insensitively, as its builder
/// documents, so it passes in whatever case either side spells it — and
/// another origin in the same case does not.
#[test]
fn a_trusted_origin_is_matched_whatever_its_case() {
    let csrf = Csrf::new().trusting_origin("https://Admin.Example.com");
    let cross_site = |origin| [("sec-fetch-site", "cross-site"), ("origin", origin)];

    for origin in [
        "https://admin.example.com",
        "HTTPS://ADMIN.EXAMPLE.COM",
        "https://Admin.Example.com",
    ] {
        assert!(
            permits(&csrf, &Method::POST, &cross_site(origin)),
            "{origin}"
        );
    }
    assert!(!permits(
        &csrf,
        &Method::POST,
        &cross_site("HTTPS://EVIL.EXAMPLE.COM")
    ));
}

/// What `Csrf` answers on the wire, where the authority it compares comes
/// from the request target rather than from an argument.
mod wire {
    use http_body_util::BodyExt;

    use crate::{
        Router,
        extract::body::text::Text,
        http::{Request, StatusCode, body::Body, header},
        middleware::csrf::Csrf,
        openapi::{Method, PathTemplate},
        router::{endpoint::builder::EndpointBuilder, service::Service},
    };

    async fn submit() -> Text {
        Text("accepted".to_owned())
    }

    fn service() -> Service<()> {
        let endpoint = EndpointBuilder::new(
            Method::Post,
            PathTemplate::parse("/submit").expect("a valid path"),
            submit,
        );
        Router::<()>::new()
            .mount(endpoint)
            .intercept(Csrf::new())
            .build(())
            .expect("a describable router")
    }

    /// A `POST` in absolute form to `api.example.com`, its `Host` naming
    /// another authority, from `origin` by a browser sending no
    /// `Sec-Fetch-Site`.
    fn absolute(origin: &str) -> Request {
        http::Request::builder()
            .method("POST")
            .uri("http://api.example.com/submit")
            .header(header::HOST, "other.example.com")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .expect("a well-formed request")
    }

    /// RFC 9112 section 3.2.2: the target's authority outranks `Host`, so a
    /// request from the target's own origin is served.
    #[tokio::test]
    async fn the_target_authority_admits_its_own_origin() {
        let response = service().call(absolute("http://api.example.com")).await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    /// And one whose origin matches only the ignored `Host` is refused with
    /// the 403 problem `Csrf` declares, before the handler runs.
    #[tokio::test]
    async fn an_origin_matching_only_the_host_field_is_refused() {
        let response = service().call(absolute("http://other.example.com")).await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .map(crate::http::HeaderValue::as_bytes),
            Some(b"application/problem+json".as_slice())
        );
        let body = response
            .into_body()
            .collect()
            .await
            .expect("a complete body")
            .to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("a problem document"),
            serde_json::json!({
                "type": "about:blank",
                "title": "Forbidden",
                "status": 403,
                "detail": "this request came from another site",
            })
        );
    }
}
