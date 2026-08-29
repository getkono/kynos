//! Every outcome one request can reach, end to end.
//!
//! One reason: routing is runtime I/O, and `docs/testing.md` allocates that an
//! integration test over the built service rather than unit tests of the
//! branches inside it. `allow_header`, `flipped` and `intern` are all reachable
//! from here, so none of them gets a test of its own — a unit test of a private
//! helper would assert the same thing twice and would keep passing if the
//! dispatcher stopped calling it.

#![cfg(all(feature = "macros", feature = "json"))]

use kynos::{
    Router,
    extract::{
        body::text::Text,
        connection::{ConnectInfo, MatchedPath},
        params::path::Path,
    },
    http::{Method, StatusCode, header},
    response::status::NoContent,
    router::policy::{FallbackPolicy, TrailingSlashPolicy},
};

#[path = "support/mod.rs"]
mod support;

use support::{App, get, send, service};

// --- The four outcomes ---------------------------------------------------

/// A request that matches a path and a method reaches its operation.
#[tokio::test]
async fn a_matched_request_reaches_its_operation() {
    let reply = get(&service(), "/users/42").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["id"], 42);
    // The context reached the handler: `Pool(7)` is what `App::new` supplies.
    assert_eq!(reply.json()["name"], "user from pool 7");
}

/// A path no template matches is a 404, and the body is the shape the policy
/// names rather than a status with nothing in it.
#[tokio::test]
async fn a_path_no_template_matches_is_not_found() {
    let reply = get(&service(), "/widgets").call().await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(
        reply.field(header::CONTENT_TYPE.as_str()).as_deref(),
        Some("application/problem+json")
    );
    assert_eq!(reply.json()["status"], 404);
}

/// A path that matches with a method that does not is a 405, and RFC 9110
/// section 15.5.6 requires the `Allow` header on one.
#[tokio::test]
async fn a_method_no_operation_declares_is_refused_with_what_is_allowed() {
    let reply = send(&service(), Method::PATCH, "/users/42").call().await;

    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);

    let allow = reply.field(header::ALLOW.as_str()).expect("an Allow field");
    let mut methods: Vec<&str> = allow.split(", ").collect();
    methods.sort_unstable();

    // Exactly the two operations declared on `/users/{id}` and nothing else --
    // in particular not the `OPTIONS` a preflight would answer, which is
    // registered after the description is assembled.
    assert_eq!(methods, ["DELETE", "GET"]);
}

/// Under `Redirect` a path reaching a declared one by flipping its final slash
/// is redirected there with 308, which preserves the method and the body.
#[tokio::test]
async fn a_trailing_slash_variant_is_redirected_when_the_policy_says_so() {
    let service = support::router()
        .trailing_slashes(TrailingSlashPolicy::Redirect)
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/42/").call().await;

    assert_eq!(reply.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        reply.field(header::LOCATION.as_str()).as_deref(),
        Some("/users/42")
    );
}

/// The pass control for the redirect: the same request under the default
/// policy, differing in exactly the property under test.
#[tokio::test]
async fn a_trailing_slash_variant_is_a_plain_miss_by_default() {
    let reply = get(&service(), "/users/42/").call().await;

    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert!(reply.field(header::LOCATION.as_str()).is_none());
}

/// A redirect keeps the query string, because the replayed request has to be
/// the same request.
#[tokio::test]
async fn a_redirect_carries_the_query_it_was_given() {
    let service = support::router()
        .trailing_slashes(TrailingSlashPolicy::Redirect)
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/?limit=2").call().await;

    assert_eq!(reply.status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(
        reply.field(header::LOCATION.as_str()).as_deref(),
        Some("/users?limit=2")
    );
}

// --- What `Lenient` accepts, and what it still refuses --------------------

/// A route whose declared spelling carries the trailing slash, which is the
/// shape `router/assets` registers a directory index under.
#[kynos::get("/things/")]
async fn list_things() -> Text {
    Text("things".to_owned())
}

/// Two operations declared under both spellings of one path, to prove a
/// declared spelling is never displaced by a flipped one.
#[kynos::get("/pair")]
async fn pair_bare() -> Text {
    Text("bare".to_owned())
}

#[kynos::get("/pair/")]
async fn pair_slashed() -> Text {
    Text("slashed".to_owned())
}

/// Echoes the template it matched, to prove a flipped spelling still reports
/// the declared one.
#[kynos::get("/label")]
async fn echo_label(MatchedPath(template): MatchedPath) -> Text {
    Text(template.to_owned())
}

/// Under `Lenient` both spellings reach the operation, with no redirect in
/// between and the same body out of each.
#[tokio::test]
async fn both_spellings_reach_the_operation_under_lenient() {
    let service = support::router()
        .trailing_slashes(TrailingSlashPolicy::Lenient)
        .build(App::new())
        .expect("a describable router");

    let declared = get(&service, "/users/42").call().await;
    let flipped = get(&service, "/users/42/").call().await;

    assert_eq!(declared.status, StatusCode::OK);
    assert_eq!(flipped.status, StatusCode::OK);
    assert!(flipped.field(header::LOCATION.as_str()).is_none());
    // The capture survives the flipped spelling: `PathCaptures` borrows the
    // request path, and under `Lenient` that is the path matchit matched.
    assert_eq!(flipped.json()["id"], 42);
    assert_eq!(declared.json(), flipped.json());
}

/// The whole point of registering the flipped spelling in the match table and
/// nowhere else: the description is the one the router declared.
#[tokio::test]
async fn lenient_adds_no_paths_key() {
    let strict = support::router().openapi().expect("a describable router");
    let lenient = support::router()
        .trailing_slashes(TrailingSlashPolicy::Lenient)
        .openapi()
        .expect("a describable router");

    let keys: Vec<&String> = lenient.paths.items.keys().collect();

    assert_eq!(
        keys,
        strict.paths.items.keys().collect::<Vec<_>>(),
        "a policy that only chooses what to match changed what is described"
    );
    assert!(!keys.iter().any(|key| key.ends_with('/')), "{keys:?}");
}

/// The direction that motivated this: a path declared *with* a slash is a
/// 404 under `Strict` when asked for without one.
#[tokio::test]
async fn a_route_declared_with_a_slash_is_reachable_without_one() {
    let service = Router::<()>::new()
        .mount(kynos::routes![list_things])
        .trailing_slashes(TrailingSlashPolicy::Lenient)
        .build(())
        .expect("a describable router");

    assert_eq!(get(&service, "/things/").call().await.text(), "things");
    assert_eq!(get(&service, "/things").call().await.text(), "things");
}

/// The pass control for the one above, differing in exactly the policy.
#[tokio::test]
async fn a_route_declared_with_a_slash_is_a_plain_miss_by_default() {
    let service = Router::<()>::new()
        .mount(kynos::routes![list_things])
        .build(())
        .expect("a describable router");

    assert_eq!(get(&service, "/things/").call().await.text(), "things");
    assert_eq!(
        get(&service, "/things").call().await.status,
        StatusCode::NOT_FOUND
    );
}

/// An application that declares both spellings keeps both. The flipped
/// spellings are registered second and collide, and what was declared stands.
#[tokio::test]
async fn a_declared_spelling_is_never_displaced_by_a_flipped_one() {
    let service = Router::<()>::new()
        .mount(kynos::routes![pair_bare, pair_slashed])
        .trailing_slashes(TrailingSlashPolicy::Lenient)
        .build(())
        .expect("a describable router");

    assert_eq!(get(&service, "/pair").call().await.text(), "bare");
    assert_eq!(get(&service, "/pair/").call().await.text(), "slashed");
}

/// Both spellings share one entry, so the label a metric is built from stays
/// the declared template rather than becoming one per spelling.
#[tokio::test]
async fn a_flipped_spelling_reports_the_declared_template() {
    let service = Router::<()>::new()
        .mount(kynos::routes![echo_label])
        .trailing_slashes(TrailingSlashPolicy::Lenient)
        .build(())
        .expect("a describable router");

    assert_eq!(get(&service, "/label").call().await.text(), "/label");
    assert_eq!(get(&service, "/label/").call().await.text(), "/label");
}

/// `Lenient` chooses what a path matches and never what a method may do, so a
/// flipped spelling reaches the same 405 and the same `Allow`.
#[tokio::test]
async fn a_flipped_spelling_still_refuses_a_method_no_operation_declares() {
    let service = support::router()
        .trailing_slashes(TrailingSlashPolicy::Lenient)
        .build(App::new())
        .expect("a describable router");

    let reply = send(&service, Method::PATCH, "/users/42/").call().await;

    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);

    // Sorted before comparing, as the declared spelling's own 405 test does:
    // `Allow` reports a set, and its order is the order operations were
    // mounted rather than anything a client may rely on.
    let allow = reply.field(header::ALLOW.as_str()).expect("an Allow field");
    let mut methods: Vec<&str> = allow.split(", ").collect();
    methods.sort_unstable();

    assert_eq!(methods, ["DELETE", "GET"]);
}

// --- What the fallback policies choose -----------------------------------

/// A policy chooses the body shape and never the status. Both fallbacks are
/// covered, because `Empty` and `Problem` are the whole of the enumeration.
#[tokio::test]
async fn an_empty_fallback_sends_the_status_and_nothing_else() {
    let service = support::router()
        .not_found(FallbackPolicy::Empty)
        .method_not_allowed(FallbackPolicy::Empty)
        .build(App::new())
        .expect("a describable router");

    let missing = get(&service, "/widgets").call().await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert!(missing.body.is_empty(), "{:?}", missing.text());

    let refused = send(&service, Method::PATCH, "/users/42").call().await;
    assert_eq!(refused.status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(refused.body.is_empty(), "{:?}", refused.text());

    // The status is not the policy's to choose, so `Allow` survives the shape.
    assert!(refused.field(header::ALLOW.as_str()).is_some());
}

// --- `MatchedPath` cardinality -------------------------------------------

/// The one thing `MatchedPath` exists to promise.
///
/// It is documented as the `paths` key precisely so that a metric label or a
/// log field built from it has bounded cardinality. A dispatcher inserting the
/// concrete URI instead would satisfy every other test here and would turn one
/// label into one per user id.
#[kynos::get("/echo/{id}")]
async fn echo_matched_path(
    Path(_): Path<support::UserPath>,
    MatchedPath(template): MatchedPath,
) -> Text {
    Text(template.to_owned())
}

#[tokio::test]
async fn the_matched_path_is_the_template_and_not_the_request_target() {
    let service = Router::<()>::new()
        .mount(kynos::routes![echo_matched_path])
        .build(())
        .expect("a describable router");

    let first = get(&service, "/echo/1").call().await;
    let second = get(&service, "/echo/99999").call().await;

    assert_eq!(first.text(), "/echo/{id}");
    assert_eq!(
        first.text(),
        second.text(),
        "two concrete paths under one template produced two labels"
    );
}

// --- The one operation that declares no body ------------------------------

/// A 204 carries no body, so a handler returning one has nothing to negotiate.
#[kynos::get("/nothing")]
async fn nothing() -> NoContent {
    NoContent
}

#[tokio::test]
async fn an_operation_returning_no_content_sends_no_body() {
    let service = Router::<()>::new()
        .mount(kynos::routes![nothing])
        .build(())
        .expect("a describable router");

    let reply = get(&service, "/nothing").call().await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert!(reply.body.is_empty());
}

// --- A service with no socket under it ------------------------------------

/// A directly-driven service still answers a handler that asks who connected.
///
/// [`Service::call`](kynos::router::service::Service::call) is public precisely
/// so a test, or an embedding owning its own accept loop, can drive a built
/// service — `examples/testing.rs` is built on it, and so is every target in
/// this directory. There is no socket there, so `ConnectInfo` has to report the
/// in-process case rather than panic on an extension nothing inserted.
#[kynos::get("/who")]
async fn who(peer: ConnectInfo) -> Text {
    Text(peer.0.to_string())
}

#[tokio::test]
async fn a_directly_driven_service_reports_an_in_process_connection() {
    let service = Router::<()>::new()
        .mount(kynos::routes![who])
        .build(())
        .expect("a describable router");

    let reply = get(&service, "/who").call().await;

    assert_eq!(reply.status, StatusCode::OK);
    // Port zero is never a peer port, so the value reads as "no socket" rather
    // than as an address a reader might try to connect back to.
    assert!(
        reply.text().ends_with(":0"),
        "an in-process connection reported `{}`",
        reply.text()
    );
}
