//! Cross-origin sharing, from the configuration to what a browser receives.
//!
//! Two things no other test target covers: that a CORS configuration the
//! protocol forbids is refused while the router is built rather than served as
//! a header browsers reject, and that the response headers reach the wire with
//! the `Vary` a shared cache needs.

#![cfg(all(feature = "macros", feature = "json"))]
#![allow(dead_code)]

use kynos::{
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    middleware::cors::Cors,
    prelude::*,
    response::status::NoContent,
    router::service::Service,
};

/// Something to mount, so the router has an operation to cover.
#[kynos::get("/widgets")]
async fn list_widgets() -> NoContent {
    NoContent
}

/// A second method on the same path, so `Allow` has more than one name in it.
#[kynos::delete("/widgets")]
async fn delete_widget() -> NoContent {
    NoContent
}

fn router() -> Router<()> {
    Router::<()>::new().mount(kynos::routes![list_widgets, delete_widget])
}

/// Drives a built service directly.
///
/// `Service::call` rather than `TestClient`, because `test-util` is not a
/// default feature and this file has to run under `mise run test:baseline`.
async fn send(
    service: &Service<()>,
    method: Method,
    path: &str,
    fields: &[(&str, &str)],
) -> (StatusCode, HeaderMap) {
    let mut request = Request::new(kynos::http::body::Body::empty());
    *request.method_mut() = method;
    *request.uri_mut() = path.parse().expect("a usable path");

    for (name, value) in fields {
        request.headers_mut().insert(
            header::HeaderName::from_bytes(name.as_bytes()).expect("a usable field name"),
            HeaderValue::from_str(value).expect("a usable field value"),
        );
    }

    let response = service.call(request).await;

    (response.status(), response.headers().clone())
}

/// The field names a preflight carries, as a lowercase set.
fn field(fields: &HeaderMap, name: header::HeaderName) -> Option<String> {
    fields
        .get(name)
        .map(|value| value.to_str().expect("a printable field").to_owned())
}

/// The CORS protocol forbids `Access-Control-Allow-Origin: *` on a credentialed
/// response, so the pair is a configuration no service can honour.
///
/// Refused while the router is built, because the alternative is worse than a
/// rejected header: `permits` short-circuits on `any_origin`, so the pair
/// silently becomes reflect-any-origin-with-credentials — the most permissive
/// CORS configuration there is, reached by asking for something else.
#[test]
fn a_router_permitting_any_origin_with_credentials_refuses_to_build() {
    let refused = router()
        .intercept(Cors::new().allow_any_origin().allow_credentials())
        .build(());

    assert!(
        refused.is_err(),
        "a wildcard origin with credentials was accepted"
    );
}

/// The refusal is in `describe`, not `build`, so every entry point that
/// assembles a description reports it rather than only the one that serves.
#[test]
fn the_same_router_reports_the_conflict_from_validate_as_well() {
    let refused = router()
        .intercept(Cors::new().allow_any_origin().allow_credentials())
        .validate();

    assert!(refused.is_err(), "validate accepted what build must refuse");
}

/// The pass control: the same router, differing in exactly the property under
/// test. Named origins with credentials is the ordinary credentialed
/// deployment and has to keep working.
#[test]
fn permitting_named_origins_with_credentials_builds() {
    router()
        .intercept(
            Cors::new()
                .allow_origins(["https://app.example.com"])
                .allow_credentials(),
        )
        .build(())
        .expect("named origins with credentials is a legal configuration");
}

/// The other half of the control: a wildcard origin *without* credentials is
/// the ordinary public-API deployment, and is not what the refusal is about.
#[test]
fn permitting_any_origin_without_credentials_builds() {
    router()
        .intercept(Cors::new().allow_any_origin())
        .build(())
        .expect("a wildcard origin alone is a legal configuration");
}

/// A browser sends `OPTIONS` before a cross-origin request that is not simple,
/// and nothing in the application declares an operation for it. The router
/// answers it.
///
/// Before this, `OPTIONS /widgets` fell through to the method-not-allowed arm:
/// the four preflight builders were stored and never read, and `#[allow(dead_code)]`
/// on each was the proof.
#[tokio::test]
async fn a_preflight_is_answered_where_the_path_declares_no_options_operation() {
    let service = router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .build(())
        .expect("a describable router");

    let (status, fields) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://app.example.com"),
            ("access-control-request-method", "DELETE"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
        Some("https://app.example.com")
    );

    let methods = field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS).expect("the method list");
    assert!(methods.contains("GET"), "{methods}");
    assert!(methods.contains("DELETE"), "{methods}");
}

/// The pass control: the same request against the same router with no CORS
/// mounted keeps the answer it has always had.
#[tokio::test]
async fn a_plain_options_request_answers_exactly_as_it_did_before_cors_was_mounted() {
    let bare = router().build(()).expect("a describable router");
    let covered = router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .build(())
        .expect("a describable router");

    // No `Origin`, no `Access-Control-Request-Method`: not a preflight.
    let (bare_status, bare_fields) = send(&bare, Method::OPTIONS, "/widgets", &[]).await;
    let (covered_status, covered_fields) = send(&covered, Method::OPTIONS, "/widgets", &[]).await;

    assert_eq!(bare_status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(covered_status, bare_status);
    assert_eq!(
        field(&covered_fields, header::ALLOW),
        field(&bare_fields, header::ALLOW)
    );
}

/// A browser sends a preflight with no credentials at all, so an interceptor
/// that refused it would break CORS for every operation on the path. A
/// preflight is not an operation, and the chain covers operations.
#[tokio::test]
async fn a_preflight_reaches_no_interceptor_that_could_refuse_it() {
    use kynos::middleware::limits::BodySize;

    // `BodySize` short-circuits with a 413 on a declared length over its limit.
    // A preflight that ran the chain would meet it.
    let service = router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .intercept(BodySize::new(1))
        .build(())
        .expect("a describable router");

    let (status, _) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://app.example.com"),
            ("access-control-request-method", "DELETE"),
            ("content-length", "4096"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// `Allow` names the operations the description declares, and a synthesized
/// preflight is in neither. A 405 that advertised `OPTIONS` would promise an
/// operation no `paths` key holds.
#[tokio::test]
async fn the_allow_header_on_a_405_never_names_the_synthesized_options() {
    let service = router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .build(())
        .expect("a describable router");

    let (status, fields) = send(&service, Method::POST, "/widgets", &[]).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let allow = field(&fields, header::ALLOW).expect("an Allow header");
    assert!(!allow.contains("OPTIONS"), "{allow}");
}

/// A preflight contributes nothing to the description: it is registered after
/// `describe` has run, so there is no point at which a `paths` key could be
/// minted from one.
#[test]
fn the_description_gains_no_options_operation_from_a_preflight() {
    let document = router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .openapi()
        .expect("a describable router");

    let emitted = serde_json::to_string(&document).expect("a serializable document");

    assert!(
        !emitted.contains("\"options\""),
        "a preflight reached the description: {emitted}"
    );
}

/// A real cross-origin response carries `Vary: Origin`, or a shared cache will
/// hand one origin's `Access-Control-Allow-Origin` to another.
#[tokio::test]
async fn a_cross_origin_response_varies_on_the_origin_it_answered() {
    let service = router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .build(())
        .expect("a describable router");

    let (_, fields) = send(
        &service,
        Method::GET,
        "/widgets",
        &[("origin", "https://app.example.com")],
    )
    .await;

    let vary = field(&fields, header::VARY).expect("a Vary header");
    assert!(
        vary.split(',').any(|name| name.trim() == "origin"),
        "{vary}"
    );
}

/// A group-scoped `Cors` advertises the methods *that group* declares, not
/// every method on the path. Scope in the router is scope in the answer, which
/// is the property shape (a) of the design could not have preserved.
#[tokio::test]
async fn a_group_scoped_cors_advertises_only_the_methods_it_covers() {
    let service = Router::<()>::new()
        .mount(kynos::routes![delete_widget])
        .group(
            kynos::router::group::Group::new("/")
                .mount(kynos::routes![list_widgets])
                .intercept(Cors::new().allow_origins(["https://app.example.com"])),
        )
        .build(())
        .expect("a describable router");

    let (status, fields) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://app.example.com"),
            ("access-control-request-method", "GET"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    let methods = field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS).expect("the method list");

    assert!(methods.contains("GET"), "{methods}");
    assert!(
        !methods.contains("DELETE"),
        "advertised a method the covering scope does not hold: {methods}"
    );
}

/// A known limit, characterized rather than left to be discovered.
///
/// An endpoint-scoped interceptor stays inside the endpoint — that is what runs
/// it — so it never reaches `Served::interceptors`, which is where preflight
/// registration looks. Hoisting it would move it out from under the endpoint's
/// own `catch_panics`, which is a real behaviour change for a case nobody has
/// asked for.
///
/// `docs/testing.md`: a documented gap is characterized, so that closing it
/// turns something red rather than nothing.
#[tokio::test]
async fn a_cors_mounted_on_one_endpoint_answers_no_preflight() {
    use kynos::{openapi, router::endpoint::builder::EndpointBuilder};

    let endpoint = EndpointBuilder::<(), _, _>::new(
        openapi::Method::Get,
        openapi::PathTemplate::parse("/widgets").expect("a valid path"),
        list_widgets_handler,
    )
    .intercept(Cors::new().allow_origins(["https://app.example.com"]));

    let service = Router::<()>::new()
        .mount(endpoint)
        .build(())
        .expect("a describable router");

    let (status, _) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://app.example.com"),
            ("access-control-request-method", "GET"),
        ],
    )
    .await;

    assert_eq!(
        status,
        StatusCode::METHOD_NOT_ALLOWED,
        "endpoint-scoped CORS started answering preflights; that is an improvement, and this \
         characterization is what should change"
    );
}

/// The plain `async fn` behind the endpoint-scoped fixture above, since a route
/// attribute expands into a type rather than a callable.
async fn list_widgets_handler() -> NoContent {
    NoContent
}
