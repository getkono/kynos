//! What the escape hatches actually do at run time.
//!
//! One reason, and it is a gap rather than a nicety: `crates/kynos/tests/`
//! contained no behavioural coverage of `route_unchecked`, `layer_unchecked` or
//! `upgrade_unchecked`, and `src/unchecked.rs` has no `#[cfg(test)]` module.
//! Nothing asserted that a catch-all serves a request, that interceptors run
//! over one, or that `x-kynos-opaque-routes` reaches a router-produced
//! document. Static assets are about to rest on all three.

#![cfg(all(feature = "unchecked", feature = "macros", feature = "json"))]

use kynos::{
    Router,
    http::{Method, Request, Response, StatusCode, body::Body},
    middleware::request_id::RequestId,
    openapi::OpaqueRoute,
};

#[path = "support/mod.rs"]
mod support;

use support::get;

/// Serves the path it was reached by, so a test can see what the router
/// captured.
async fn echo_path(request: Request) -> Response {
    let mut response = Response::new(Body::from_bytes(bytes::Bytes::from(
        request.uri().path().to_owned(),
    )));
    *response.status_mut() = StatusCode::OK;
    response
}

/// A router serving one catch-all under `/static`.
fn with_catch_all() -> Router<()> {
    Router::<()>::new().route_unchecked([Method::GET], "/static/{*path}", echo_path)
}

// --- That it serves at all ------------------------------------------------

/// A catch-all reaches its handler, at any depth.
#[tokio::test]
async fn a_catch_all_serves_every_path_beneath_its_prefix() {
    let service = with_catch_all().build(()).expect("a buildable router");

    for path in [
        "/static/app.css",
        "/static/css/app.css",
        "/static/a/b/c.png",
    ] {
        let reply = get(&service, path).call().await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert_eq!(reply.text(), path);
    }
}

/// And nothing outside it.
#[tokio::test]
async fn a_catch_all_does_not_answer_outside_its_prefix() {
    let service = with_catch_all().build(()).expect("a buildable router");

    assert_eq!(
        get(&service, "/elsewhere").call().await.status,
        StatusCode::NOT_FOUND
    );
}

/// matchit's catch-all does not match the prefix itself, or the prefix with a
/// trailing slash.
///
/// Recorded rather than fixed: it is the matcher's rule, and a service wanting
/// an index at the mount root registers one. Closing this would turn a test red
/// rather than nothing.
#[tokio::test]
async fn a_catch_all_matches_neither_its_own_prefix_nor_a_bare_trailing_slash() {
    let service = with_catch_all().build(()).expect("a buildable router");

    for path in ["/static", "/static/"] {
        assert_eq!(
            get(&service, path).call().await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

// --- That the router's own machinery still runs ---------------------------

/// Router-scoped interceptors run over an unchecked route.
///
/// `route_unchecked`'s documentation says so; nothing checked it.
#[tokio::test]
async fn a_router_interceptor_covers_an_unchecked_route() {
    let service = with_catch_all()
        .intercept(RequestId::new())
        .build(())
        .expect("a buildable router");

    let reply = get(&service, "/static/app.css").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        reply.field("x-request-id").is_some(),
        "the router's interceptor did not cover the unchecked route"
    );
}

// --- What the document says -----------------------------------------------

/// The route is recorded at the document root, and gets no `paths` key.
#[test]
fn an_unchecked_route_is_recorded_where_a_generator_cannot_act_on_it() {
    let document = with_catch_all().openapi().expect("a describable router");

    assert!(
        document.paths.0.is_empty(),
        "a catch-all took a `paths` key, which is a claim about a path it does not honour"
    );

    let recorded = OpaqueRoute::all(&document).expect("a readable record");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].pattern, "/static/{*path}");
    assert_eq!(recorded[0].prefix.as_deref(), Some("/static"));
    assert_eq!(recorded[0].methods, ["GET"]);

    assert!(!document.is_authoritative());
}
