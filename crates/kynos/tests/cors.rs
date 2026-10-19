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
        matches!(
            refused,
            Err(kynos::Error::Middleware(
                kynos::middleware::MiddlewareError::CredentialedWildcardOrigin
            ))
        ),
        "a wildcard origin with credentials was not refused as one"
    );
}

/// The refusal is in `describe`, not `build`, so every entry point that
/// assembles a description reports it rather than only the one that serves.
#[test]
fn the_same_router_reports_the_conflict_from_validate_as_well() {
    let refused = router()
        .intercept(Cors::new().allow_any_origin().allow_credentials())
        .validate();

    assert!(
        matches!(
            refused,
            Err(kynos::Error::Middleware(
                kynos::middleware::MiddlewareError::CredentialedWildcardOrigin
            ))
        ),
        "validate did not refuse what build must refuse"
    );
}

/// The second conflict, through a router: exposing every header beside
/// credentials is read by the protocol as exposing a header literally named
/// `*`, so it exposes nothing and is refused rather than shipped.
#[test]
fn a_router_exposing_every_header_with_credentials_refuses_to_build() {
    let refused = router()
        .intercept(
            Cors::new()
                .allow_origins(["https://app.example.com"])
                .expose_any_header()
                .allow_credentials(),
        )
        .build(());

    assert!(
        matches!(
            refused,
            Err(kynos::Error::Middleware(
                kynos::middleware::MiddlewareError::CredentialedWildcardExposure
            ))
        ),
        "every header exposed with credentials was not refused as such"
    );
}

/// Its control: the same configuration without credentials, where the
/// wildcard means what it says.
#[test]
fn a_router_exposing_every_header_without_credentials_builds() {
    router()
        .intercept(
            Cors::new()
                .allow_origins(["https://app.example.com"])
                .expose_any_header(),
        )
        .build(())
        .expect("exposing every header to an uncredentialed request is legal");
}

/// The refusal has no scope exception: the endpoint's own `Cors` reflects
/// every origin with credentials just as a router's would.
#[test]
fn an_endpoint_permitting_any_origin_with_credentials_refuses_to_build() {
    let refused = Router::<()>::new()
        .mount(kynos::routes![
            list_widgets.intercept(Cors::new().allow_any_origin().allow_credentials())
        ])
        .build(());

    assert!(
        matches!(
            refused,
            Err(kynos::Error::Middleware(
                kynos::middleware::MiddlewareError::CredentialedWildcardOrigin
            ))
        ),
        "an endpoint-scoped wildcard origin with credentials was accepted"
    );
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

    // Every covered method, and the HEAD the covered GET answers.
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS).as_deref(),
        Some("GET, HEAD, DELETE")
    );
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

    // Nothing in the service implements `OPTIONS`: the preflight CORS
    // registers is not an operation, so it does not turn this into a 405.
    assert_eq!(bare_status, StatusCode::NOT_IMPLEMENTED);
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
    use kynos::middleware::limits::body_size::BodySize;

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

/// A `POST` somewhere else in the service, so a `POST` to `/widgets` is a
/// method implemented but not allowed there -- a 405 rather than a 501.
#[kynos::post("/gadgets")]
async fn create_gadget() -> NoContent {
    NoContent
}

/// `Allow` names the operations the description declares and the `HEAD` each
/// `GET` answers, and a synthesized preflight is neither. A 405 that
/// advertised `OPTIONS` would promise an operation no `paths` key holds.
#[tokio::test]
async fn the_allow_header_on_a_405_never_names_the_synthesized_options() {
    let service = router()
        .mount(kynos::routes![create_gadget])
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .build(())
        .expect("a describable router");

    let (status, fields) = send(&service, Method::POST, "/widgets", &[]).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        field(&fields, header::ALLOW).as_deref(),
        Some("GET, HEAD, DELETE"),
        "Allow named other than the declared methods and the implied HEAD"
    );
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

    assert_eq!(field(&fields, header::VARY).as_deref(), Some("origin"));
}

/// A group-scoped `Cors` advertises the methods *that group* covers — its
/// declared methods and the `HEAD` each `GET` answers — not every method on
/// the path. Scope in the router is scope in the answer, which
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
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS).as_deref(),
        Some("GET, HEAD"),
        "advertised other than the GET the group covers and the HEAD it answers"
    );
}

/// A method the path answers outside every `Cors` is refused on preflight, even
/// where a covering scope's `allow_methods` names it.
///
/// The override is for routes Kynos does not serve. Here Kynos does serve
/// `DELETE`, under no `Cors` at all, so approving the preflight would send the
/// browser on to a request whose side effect runs while its response carries no
/// CORS header.
#[tokio::test]
async fn a_preflight_refuses_a_served_method_no_cors_covers() {
    let service = Router::<()>::new()
        .mount(kynos::routes![delete_widget])
        .group(
            kynos::router::group::Group::new("/")
                .mount(kynos::routes![list_widgets])
                .intercept(
                    Cors::new()
                        .allow_origins(["https://app.example.com"])
                        .allow_methods([
                            kynos::openapi::Method::Get,
                            kynos::openapi::Method::Delete,
                        ]),
                ),
        )
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
    assert!(
        field(&fields, header::VARY).is_some(),
        "a refusal is cached too"
    );
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        None,
        "approved a preflight for a method no CORS configuration covers"
    );
    assert_eq!(field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS), None);
}

/// A third method on the same path, for a `Cors` covering neither of the others.
#[kynos::post("/widgets")]
async fn create_widget() -> NoContent {
    NoContent
}

/// The `HEAD` a `GET` answers counts as served, so it runs under the `GET`'s
/// chain: where that chain holds no `Cors`, a `HEAD` preflight is refused even
/// though a sibling scope's `allow_methods` names it.
#[tokio::test]
async fn a_preflight_refuses_the_head_an_uncovered_get_answers() {
    let service = Router::<()>::new()
        .mount(kynos::routes![list_widgets])
        .group(
            kynos::router::group::Group::new("/")
                .mount(kynos::routes![create_widget])
                .intercept(
                    Cors::new()
                        .allow_origins(["https://app.example.com"])
                        .allow_methods([kynos::openapi::Method::Head]),
                ),
        )
        .build(())
        .expect("a describable router");

    let (status, fields) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://app.example.com"),
            ("access-control-request-method", "HEAD"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        None,
        "approved a preflight for the HEAD an uncovered GET answers"
    );
    assert_eq!(field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS), None);
}

/// Only a `GET` makes a `HEAD` served: on a path with none, a `HEAD` is a
/// method Kynos does not serve, so an `allow_methods` override naming it
/// approves and advertises it as it would any route fronted elsewhere.
#[tokio::test]
async fn an_override_approves_a_head_on_a_path_serving_no_get() {
    let service = Router::<()>::new()
        .mount(kynos::routes![create_widget])
        .intercept(
            Cors::new()
                .allow_origins(["https://app.example.com"])
                .allow_methods([kynos::openapi::Method::Post, kynos::openapi::Method::Head]),
        )
        .build(())
        .expect("a describable router");

    let (status, fields) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://app.example.com"),
            ("access-control-request-method", "HEAD"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
        Some("https://app.example.com"),
        "refused a HEAD the path does not serve though the override names it"
    );
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS).as_deref(),
        Some("POST, HEAD")
    );
}

/// A predicate reaches both answers a browser sees: the preflight, and the
/// real response. Two places read the allow-list, so a widening that only one
/// of them honoured would let a preflight pass and the request that followed
/// it fail.
#[tokio::test]
async fn a_predicate_permits_an_origin_on_the_preflight_and_the_response_alike() {
    let service = router()
        .intercept(Cors::new().allow_origins_matching(|origin| origin.ends_with(".example.com")))
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
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
        Some("https://app.example.com"),
        "the preflight refused an origin the predicate accepts"
    );

    let (_, fields) = send(
        &service,
        Method::GET,
        "/widgets",
        &[("origin", "https://app.example.com")],
    )
    .await;

    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
        Some("https://app.example.com"),
        "the response refused an origin the preflight had already promised"
    );

    let (_, fields) = send(
        &service,
        Method::GET,
        "/widgets",
        &[("origin", "https://app.example.net")],
    )
    .await;

    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        None,
        "permitted an origin the predicate rejects"
    );
}

/// Two groups covering one path each get their own answer.
///
/// `Group`'s stack is checked against the *router's*, and never against a
/// sibling's — the premise being that no request reaches two operations. A
/// preflight is the one request that does: it is answered per path, while the
/// configuration that permits an origin is mounted per operation. So the two
/// `Cors` values below compile, and the answer to a `DELETE` preflight has to
/// come from the one covering `DELETE` rather than from whichever was mounted
/// first.
#[tokio::test]
async fn a_preflight_answers_from_the_cors_covering_the_method_it_proposes() {
    use kynos::router::group::Group;

    let service = Router::<()>::new()
        .group(
            Group::new("/")
                .mount(kynos::routes![list_widgets])
                .intercept(Cors::new().allow_origins(["https://reader.example.com"])),
        )
        .group(
            Group::new("/")
                .mount(kynos::routes![delete_widget])
                .intercept(Cors::new().allow_origins(["https://admin.example.com"])),
        )
        .build(())
        .expect("a describable router");

    let (status, fields) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://admin.example.com"),
            ("access-control-request-method", "DELETE"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
        Some("https://admin.example.com"),
        "answered a DELETE preflight from the configuration covering GET"
    );

    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS).as_deref(),
        Some("DELETE"),
        "advertised a method the covering scope does not hold"
    );
}

/// The mirror of the case above: the origin the *other* scope permits is not
/// permitted for this method, and a preflight that said otherwise would send
/// the browser on to a request whose real response carries no CORS header.
#[tokio::test]
async fn a_preflight_refuses_an_origin_the_covering_cors_does_not_permit() {
    use kynos::router::group::Group;

    let service = Router::<()>::new()
        .group(
            Group::new("/")
                .mount(kynos::routes![list_widgets])
                .intercept(Cors::new().allow_origins(["https://reader.example.com"])),
        )
        .group(
            Group::new("/")
                .mount(kynos::routes![delete_widget])
                .intercept(Cors::new().allow_origins(["https://admin.example.com"])),
        )
        .build(())
        .expect("a describable router");

    let (status, fields) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://reader.example.com"),
            ("access-control-request-method", "DELETE"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN),
        None,
        "permitted an origin only the GET scope names"
    );
}

/// A HEAD on a path declaring no `head` runs under the `GET` operation's
/// chain, so its preflight is answered by the `Cors` covering `GET`.
///
/// The Fetch standard preflights a HEAD like any other method once it carries
/// a header outside the safelist, such as `Authorization`. The group covering
/// `DELETE` is mounted first, so a preflight that matched declared methods
/// only would fall back to it and permit no origin the real HEAD is served to.
#[tokio::test]
async fn a_head_preflight_answers_from_the_cors_covering_get() {
    use kynos::router::group::Group;

    let service = Router::<()>::new()
        .group(
            Group::new("/")
                .mount(kynos::routes![delete_widget])
                .intercept(Cors::new().allow_origins(["https://admin.example.com"])),
        )
        .group(
            Group::new("/")
                .mount(kynos::routes![list_widgets])
                .intercept(Cors::new().allow_origins(["https://reader.example.com"])),
        )
        .build(())
        .expect("a describable router");

    let (status, fields) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://reader.example.com"),
            ("access-control-request-method", "HEAD"),
            ("access-control-request-headers", "authorization"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_ORIGIN).as_deref(),
        Some("https://reader.example.com"),
        "answered a HEAD preflight from a configuration not covering GET"
    );

    // Advertised the way `Allow` names it: after the `GET` that answers it.
    assert_eq!(
        field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS).as_deref(),
        Some("GET, HEAD")
    );
}

/// A `head` declared beside the `GET` on `/widgets`.
#[kynos::head("/widgets")]
async fn probe_widgets() -> NoContent {
    NoContent
}

/// Where a path declares its own `head`, the GET implies no second one: a
/// preflight advertises HEAD once, exactly as the 405's `Allow` names it.
#[tokio::test]
async fn a_declared_head_is_advertised_once() {
    let service = router()
        .mount(kynos::routes![probe_widgets, create_gadget])
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .build(())
        .expect("a describable router");

    let (status, fields) = send(
        &service,
        Method::OPTIONS,
        "/widgets",
        &[
            ("origin", "https://app.example.com"),
            ("access-control-request-method", "HEAD"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let advertised =
        field(&fields, header::ACCESS_CONTROL_ALLOW_METHODS).expect("an advertised list");

    let (status, fields) = send(&service, Method::POST, "/widgets", &[]).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let allow = field(&fields, header::ALLOW).expect("an Allow header");

    assert_eq!(
        advertised
            .split(", ")
            .filter(|method| *method == "HEAD")
            .count(),
        1,
        "{advertised}"
    );
    assert_eq!(advertised, allow);
}

/// A service implementing `OPTIONS` somewhere refuses a plain `OPTIONS` on a
/// path without one with a 405 and that path's `Allow`, and mounting CORS over
/// the path changes neither.
#[kynos::options("/gadgets")]
async fn describe_gadgets() -> NoContent {
    NoContent
}

#[tokio::test]
async fn a_plain_options_stays_a_405_where_the_service_implements_options_elsewhere() {
    let bare = router()
        .mount(kynos::routes![describe_gadgets])
        .build(())
        .expect("a describable router");
    let covered = router()
        .mount(kynos::routes![describe_gadgets])
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .build(())
        .expect("a describable router");

    let (bare_status, bare_fields) = send(&bare, Method::OPTIONS, "/widgets", &[]).await;
    let (covered_status, covered_fields) = send(&covered, Method::OPTIONS, "/widgets", &[]).await;

    assert_eq!(bare_status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(covered_status, bare_status);

    let allow = field(&bare_fields, header::ALLOW).expect("an Allow header");
    assert_eq!(field(&covered_fields, header::ALLOW), Some(allow));
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
        StatusCode::NOT_IMPLEMENTED,
        "endpoint-scoped CORS started answering preflights; that is an improvement, and this \
         characterization is what should change"
    );
}

/// The plain `async fn` behind the endpoint-scoped fixture above, since a route
/// attribute expands into a type rather than a callable.
async fn list_widgets_handler() -> NoContent {
    NoContent
}

/// The `Access-Control-*` fields a response carries, sorted by name.
fn cors_fields(fields: &HeaderMap) -> Vec<(String, String)> {
    let mut found: Vec<_> = fields
        .iter()
        .filter(|(name, _)| name.as_str().starts_with("access-control-"))
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().expect("a printable field").to_owned(),
            )
        })
        .collect();
    found.sort();
    found
}

/// A case's name, its configuration, the `Origin` sent, and the fields owed.
type Case = (
    &'static str,
    Cors,
    Option<&'static str>,
    &'static [(&'static str, &'static str)],
);

/// The real-response table: each configuration, the `Origin` it is asked
/// with, and every `Access-Control-*` field it owes.
fn real_response_cases() -> Vec<Case> {
    const APP: &str = "https://app.example.com";

    let named = || Cors::new().allow_origins([APP]);

    vec![
        (
            "a named origin",
            named(),
            Some(APP),
            &[("access-control-allow-origin", APP)],
        ),
        (
            "credentials",
            named().allow_credentials(),
            Some(APP),
            &[
                ("access-control-allow-credentials", "true"),
                ("access-control-allow-origin", APP),
            ],
        ),
        (
            "an exposure list",
            named().expose_headers(["x-total", "x-page"]),
            Some(APP),
            &[
                ("access-control-allow-origin", APP),
                ("access-control-expose-headers", "x-total, x-page"),
            ],
        ),
        (
            "every header exposed",
            named().expose_any_header(),
            Some(APP),
            &[
                ("access-control-allow-origin", APP),
                ("access-control-expose-headers", "*"),
            ],
        ),
        (
            "any origin",
            Cors::new().allow_any_origin(),
            Some("https://anyone.example"),
            &[("access-control-allow-origin", "*")],
        ),
        (
            "an origin in another case",
            named(),
            Some("HTTPS://APP.EXAMPLE.COM"),
            &[("access-control-allow-origin", "HTTPS://APP.EXAMPLE.COM")],
        ),
        (
            "a predicate's origin",
            Cors::new()
                .allow_origins_matching(|origin| origin.ends_with(".example.com"))
                .allow_credentials(),
            Some("https://tenant.example.com"),
            &[
                ("access-control-allow-credentials", "true"),
                ("access-control-allow-origin", "https://tenant.example.com"),
            ],
        ),
        (
            "no Origin",
            named().allow_credentials().expose_headers(["x-total"]),
            None,
            &[],
        ),
        (
            "an origin not permitted",
            named().allow_credentials().expose_headers(["x-total"]),
            Some("https://evil.example.com"),
            &[],
        ),
        (
            "any origin, but no Origin",
            Cors::new().allow_any_origin(),
            None,
            &[],
        ),
    ]
}

/// What a real response carries for each configuration and `Origin`, read
/// field for field: the origin echoed or `*`, `Access-Control-Allow-Credentials`
/// only when credentials are allowed, the exposure list as one field, nothing
/// at all for a request with no `Origin` or one not permitted, and an origin
/// matched case-insensitively and echoed as it was sent. Every one carries
/// `Vary: origin`, granted or not, since a cache cannot see which case it
/// stored.
#[tokio::test]
async fn a_real_response_carries_exactly_the_fields_its_configuration_grants() {
    for (case, cors, origin, expected) in real_response_cases() {
        let service = router()
            .intercept(cors)
            .build(())
            .expect("a describable router");
        let request: Vec<(&str, &str)> = origin
            .map(|origin| ("origin", origin))
            .into_iter()
            .collect();

        let (status, fields) = send(&service, Method::GET, "/widgets", &request).await;

        assert_eq!(status, StatusCode::NO_CONTENT, "{case}");
        let expected: Vec<(String, String)> = expected
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        assert_eq!(cors_fields(&fields), expected, "{case}");
        assert_eq!(
            field(&fields, header::VARY).as_deref(),
            Some("origin"),
            "{case}"
        );
    }
}

/// A `Cors` that documents its response headers is still the `Cors` the
/// router reads for preflight and for its conflict check: its preflight is
/// answered, its real response carries the same fields, an unhonourable
/// configuration is refused, and the description declares the three fields
/// on the operation it covers.
mod documented {
    use kynos::{
        http::{Method, StatusCode, header},
        middleware::cors::Cors,
    };

    use super::{cors_fields, field, router, send};

    fn cors() -> kynos::middleware::cors::Cors<kynos::middleware::cors::Documented> {
        Cors::new()
            .allow_origins(["https://app.example.com"])
            .expose_headers(["x-total"])
            .allow_credentials()
            .document_response_headers()
    }

    #[tokio::test]
    async fn its_preflight_is_answered_from_its_configuration() {
        let service = router()
            .intercept(cors())
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
            cors_fields(&fields),
            [
                ("access-control-allow-credentials", "true"),
                ("access-control-allow-methods", "GET, HEAD, DELETE"),
                ("access-control-allow-origin", "https://app.example.com"),
            ]
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
        );
    }

    #[tokio::test]
    async fn its_real_response_carries_the_fields_it_declares() {
        let service = router()
            .intercept(cors())
            .build(())
            .expect("a describable router");

        let (_, fields) = send(
            &service,
            Method::GET,
            "/widgets",
            &[("origin", "https://app.example.com")],
        )
        .await;

        assert_eq!(
            cors_fields(&fields),
            [
                ("access-control-allow-credentials", "true"),
                ("access-control-allow-origin", "https://app.example.com"),
                ("access-control-expose-headers", "x-total"),
            ]
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
        );
        assert_eq!(field(&fields, header::VARY).as_deref(), Some("origin"));
    }

    #[test]
    fn an_unhonourable_configuration_is_refused_as_it_is_undocumented() {
        let refused = router()
            .intercept(
                Cors::new()
                    .allow_any_origin()
                    .allow_credentials()
                    .document_response_headers(),
            )
            .build(());

        assert!(
            matches!(
                refused,
                Err(kynos::Error::Middleware(
                    kynos::middleware::MiddlewareError::CredentialedWildcardOrigin
                ))
            ),
            "a documented wildcard origin with credentials was not refused"
        );
    }

    /// The three fields, on the response the covered operation declares, and
    /// not when the same configuration stays undocumented.
    #[test]
    fn the_description_declares_its_three_fields_on_what_it_covers() {
        let headers_of = |document: &serde_json::Value| {
            let mut names: Vec<String> =
                document["paths"]["/widgets"]["get"]["responses"]["204"]["headers"]
                    .as_object()
                    .map(|headers| headers.keys().cloned().collect())
                    .unwrap_or_default();
            names.sort();
            names
        };

        let documented = serde_json::to_value(
            router()
                .intercept(cors())
                .openapi()
                .expect("a describable router"),
        )
        .expect("a serializable document");
        let undocumented = serde_json::to_value(
            router()
                .intercept(
                    Cors::new()
                        .allow_origins(["https://app.example.com"])
                        .allow_credentials(),
                )
                .openapi()
                .expect("a describable router"),
        )
        .expect("a serializable document");

        assert_eq!(
            headers_of(&documented),
            [
                "access-control-allow-credentials",
                "access-control-allow-origin",
                "access-control-expose-headers",
            ]
        );
        assert_eq!(headers_of(&undocumented), Vec::<String>::new());
    }
}
