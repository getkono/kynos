use kynos_openapi::Method;

use super::{super::Wildcards, CorsConfig, Preflight, Scope};
use crate::{
    http::{HeaderValue, Request, StatusCode, header},
    router::policy::FallbackPolicy,
};

/// A preflight over the methods a path actually declares, all covered by one
/// configuration.
fn preflight(config: CorsConfig) -> Preflight {
    Preflight::new(
        vec![Scope::new(config, vec![Method::Get, Method::Delete])],
        HeaderValue::from_static("GET, DELETE"),
        FallbackPolicy::Problem,
    )
}

/// A configuration permitting one named origin.
fn named() -> CorsConfig {
    CorsConfig {
        origins: vec!["https://app.example.com".into()],
        ..CorsConfig::default()
    }
}

/// An `OPTIONS` carrying the fields a preflight is required to carry.
fn asking(origin: Option<&str>, method: Option<&str>, headers: Option<&str>) -> Request {
    let mut request = Request::new(crate::http::body::Body::empty());
    *request.method_mut() = crate::http::Method::OPTIONS;

    let fields = request.headers_mut();
    if let Some(origin) = origin {
        fields.insert(header::ORIGIN, HeaderValue::from_str(origin).unwrap());
    }
    if let Some(method) = method {
        fields.insert(
            header::ACCESS_CONTROL_REQUEST_METHOD,
            HeaderValue::from_str(method).unwrap(),
        );
    }
    if let Some(headers) = headers {
        fields.insert(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            HeaderValue::from_str(headers).unwrap(),
        );
    }

    request
}

/// The value of one response header, as a string.
fn field(response: &crate::http::Response, name: header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .map(|value| value.to_str().expect("a printable field").to_owned())
}

/// Both fields are required of a preflight, so an `OPTIONS` missing `Origin` is
/// an ordinary request for a method this path does not declare — and it keeps
/// the 405 it had before CORS was mounted.
#[test]
fn a_request_without_an_origin_is_not_a_preflight() {
    let response = preflight(named()).answer(&asking(None, Some("GET"), None));

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        field(&response, header::ALLOW).as_deref(),
        Some("GET, DELETE")
    );
}

#[test]
fn a_request_without_an_access_control_request_method_is_not_a_preflight() {
    let response = preflight(named()).answer(&asking(Some("https://app.example.com"), None, None));

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        field(&response, header::ALLOW).as_deref(),
        Some("GET, DELETE")
    );
}

/// The protocol reads an absent header as a refusal, so there is nothing to
/// send and no status to invent.
#[test]
fn an_unpermitted_origin_is_answered_with_no_cors_header_at_all() {
    let response =
        preflight(named()).answer(&asking(Some("https://evil.example.com"), Some("GET"), None));

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(field(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN), None);
    assert_eq!(field(&response, header::ACCESS_CONTROL_ALLOW_METHODS), None);
}

/// Derived from the operations declared on the path, which is what stops
/// preflight advertising a method the description does not promise.
#[test]
fn a_permitted_preflight_advertises_exactly_the_methods_the_path_declares() {
    let response =
        preflight(named()).answer(&asking(Some("https://app.example.com"), Some("GET"), None));

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        field(&response, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
        Some("https://app.example.com")
    );
    assert_eq!(
        field(&response, header::ACCESS_CONTROL_ALLOW_METHODS).as_deref(),
        Some("GET, DELETE")
    );
}

/// The override is for a deployment fronting routes Kynos does not serve.
#[test]
fn an_overridden_method_list_replaces_the_declared_one() {
    let config = CorsConfig {
        methods: Some(vec![Method::Patch]),
        ..named()
    };

    let response = preflight(config).answer(&asking(
        Some("https://app.example.com"),
        Some("PATCH"),
        None,
    ));

    assert_eq!(
        field(&response, header::ACCESS_CONTROL_ALLOW_METHODS).as_deref(),
        Some("PATCH")
    );
}

/// `*` is not a wildcard on a credentialed response, so the only way to answer
/// one under `allow_any_header` is to echo what was asked for.
#[test]
fn permitting_any_header_echoes_the_request_headers_on_a_credentialed_preflight() {
    let config = CorsConfig {
        any: Wildcards {
            header: true,
            ..Wildcards::default()
        },
        credentials: true,
        ..named()
    };

    let response = preflight(config).answer(&asking(
        Some("https://app.example.com"),
        Some("GET"),
        Some("x-trace-id, authorization"),
    ));

    assert_eq!(
        field(&response, header::ACCESS_CONTROL_ALLOW_HEADERS).as_deref(),
        Some("x-trace-id, authorization")
    );
    assert_eq!(
        field(&response, header::ACCESS_CONTROL_ALLOW_CREDENTIALS).as_deref(),
        Some("true")
    );
}

/// Without credentials there is nothing to echo, and `*` says it in one field.
#[test]
fn permitting_any_header_answers_a_wildcard_without_credentials() {
    let config = CorsConfig {
        any: Wildcards {
            header: true,
            ..Wildcards::default()
        },
        ..named()
    };

    let response = preflight(config).answer(&asking(
        Some("https://app.example.com"),
        Some("GET"),
        Some("x-trace-id"),
    ));

    assert_eq!(
        field(&response, header::ACCESS_CONTROL_ALLOW_HEADERS).as_deref(),
        Some("*")
    );
}

/// A cache that keyed on origin alone would hand a `PUT` preflight's answer to
/// a `DELETE`, so all three fields the answer read are named.
#[test]
fn a_preflight_varies_on_the_three_fields_it_read() {
    let response =
        preflight(named()).answer(&asking(Some("https://app.example.com"), Some("GET"), None));

    let vary = field(&response, header::VARY).expect("a Vary");
    let names: Vec<_> = vary.split(',').map(str::trim).collect();

    assert!(names.contains(&"origin"), "{vary}");
    assert!(names.contains(&"access-control-request-method"), "{vary}");
    assert!(names.contains(&"access-control-request-headers"), "{vary}");
}

/// A refusal is cached too, so it varies on the same fields a permission does.
#[test]
fn a_refused_preflight_varies_the_same_way_a_permitted_one_does() {
    let response =
        preflight(named()).answer(&asking(Some("https://evil.example.com"), Some("GET"), None));

    assert!(field(&response, header::VARY).is_some());
}

#[test]
fn a_configured_max_age_is_advertised_in_whole_seconds() {
    let config = CorsConfig {
        max_age: Some(std::time::Duration::from_secs(600)),
        ..named()
    };

    let response =
        preflight(config).answer(&asking(Some("https://app.example.com"), Some("GET"), None));

    assert_eq!(
        field(&response, header::ACCESS_CONTROL_MAX_AGE).as_deref(),
        Some("600")
    );
}
