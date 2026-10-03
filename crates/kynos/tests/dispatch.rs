//! Every outcome one request can reach, end to end.
//!
//! One reason: routing is runtime I/O, and `docs/testing.md` allocates that an
//! integration test over the built service rather than unit tests of the
//! branches inside it. `allow_header`, `flipped` and `intern` are all reachable
//! from here, so none of them gets a test of its own — a unit test of a private
//! helper would assert the same thing twice and would keep passing if the
//! dispatcher stopped calling it.

#![cfg(all(feature = "macros", feature = "json"))]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use kynos::{
    Router,
    extract::{
        body::text::Text,
        connection::{ConnectInfo, MatchedPath},
        params::path::Path,
    },
    http::{
        Method, Request, Response, StatusCode,
        forwarded::{Forwarded, TrustedProxies},
        header,
    },
    middleware::Observer,
    openapi,
    response::status::NoContent,
    router::{
        operation::Route,
        policy::{FallbackPolicy, TrailingSlashPolicy},
    },
};

#[path = "support/mod.rs"]
mod support;

use support::{App, get, send, service};

// --- The five outcomes ---------------------------------------------------

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
///
/// `POST` because the service implements it on `/users`: section 9.1 keeps the
/// 405 for a method "recognized and implemented, but not allowed for the
/// target resource".
#[tokio::test]
async fn a_method_implemented_elsewhere_is_refused_with_what_is_allowed() {
    let reply = send(&service(), Method::POST, "/users/42").call().await;

    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);

    let allow = reply.field(header::ALLOW.as_str()).expect("an Allow field");
    let mut methods: Vec<&str> = allow.split(", ").collect();
    methods.sort_unstable();

    // The two operations declared on `/users/{id}`, and the `HEAD` its `GET`
    // answers -- and nothing else, in particular not the `OPTIONS` a
    // preflight would answer, which is registered after the description is
    // assembled.
    assert_eq!(methods, ["DELETE", "GET", "HEAD"]);
}

/// A method no operation in the service implements is a 501, RFC 9110 section
/// 9.1: "An origin server that receives a request method that is unrecognized
/// or not implemented SHOULD respond with the 501 (Not Implemented) status
/// code." Section 15.6.2 makes that a property of the server rather than of a
/// resource, so it is decided before the 404 or 308 on an unmatched path and
/// instead of the 405 on a matched one: an unmatched path is no 404 and a
/// flipped spelling no 308 for a method nothing could serve. No `Allow` either
/// -- section 10.2.1 requires one on a 405 only.
#[tokio::test]
async fn a_method_no_operation_implements_is_not_implemented() {
    let frob = Method::from_bytes(b"FROB").expect("a method token");
    let strict = service();
    let redirecting = support::router()
        .trailing_slashes(TrailingSlashPolicy::Redirect)
        .build(App::new())
        .expect("a describable router");

    for (service, method, target) in [
        (&strict, Method::PATCH, "/users/42"),
        (&strict, frob.clone(), "/users/42"),
        (&strict, frob.clone(), "/widgets"),
        (&redirecting, frob, "/users/42/"),
    ] {
        let reply = send(service, method.clone(), target).call().await;
        let asked = format!("{method} {target}");

        assert_eq!(reply.status, StatusCode::NOT_IMPLEMENTED, "{asked}");
        assert_eq!(reply.field(header::ALLOW.as_str()), None, "{asked}");
        assert_eq!(
            reply.field(header::CONTENT_TYPE.as_str()).as_deref(),
            Some("application/problem+json"),
            "{asked}"
        );
        assert_eq!(reply.json()["status"], 501, "{asked}");
    }
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

// --- HEAD, which every GET answers ----------------------------------------

/// RFC 9110 section 9.1: "All general-purpose servers MUST support the methods
/// GET and HEAD." Section 9.3.2 defines a HEAD as the GET with no content, so a
/// path declaring `get` and no `head` answers one from its GET operation.
///
/// The fields are GET's, `Content-Length` included: section 8.6 lets a HEAD
/// carry the length a GET would have sent, and an emptied body would otherwise
/// state none.
#[tokio::test]
async fn a_head_is_answered_by_the_get_operation_without_content() {
    let service = service();

    let full = get(&service, "/users/42").call().await;
    let head = send(&service, Method::HEAD, "/users/42").call().await;

    assert_eq!(full.status, StatusCode::OK);
    assert_eq!(head.status, full.status);
    assert!(head.body.is_empty(), "{:?}", head.text());
    assert_eq!(
        head.field(header::CONTENT_TYPE.as_str()),
        full.field(header::CONTENT_TYPE.as_str())
    );
    assert_eq!(
        head.field(header::CONTENT_LENGTH.as_str()),
        Some(full.body.len().to_string())
    );
}

/// A path declaring both answers a HEAD from its own `head` operation. The two
/// answer with different statuses, so the reply says which one ran.
#[kynos::get("/both")]
async fn both_get() -> Text {
    Text("get".to_owned())
}

#[kynos::head("/both")]
async fn both_head() -> NoContent {
    NoContent
}

#[tokio::test]
async fn a_declared_head_wins_over_the_one_get_implies() {
    let service = Router::<()>::new()
        .mount(kynos::routes![both_get, both_head])
        .build(())
        .expect("a describable router");

    let reply = send(&service, Method::HEAD, "/both").call().await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    // RFC 9110 section 8.6: "A server MUST NOT send a Content-Length header
    // field in any response with a status code of 1xx (Informational) or 204
    // (No Content)."
    assert_eq!(reply.field(header::CONTENT_LENGTH.as_str()), None);
    assert!(reply.body.is_empty(), "{:?}", reply.text());
}

/// What an observer saw of one request: the method on the wire, and the
/// operation it was routed to.
type Sighting = (Method, Option<(openapi::Method, String, String)>);

/// Records every request's method and route. Held per test rather than in a
/// `static`, so no two tests can see each other's traffic.
#[derive(Clone, Default)]
struct Sightings(Arc<Mutex<Vec<Sighting>>>);

impl Sightings {
    fn taken(&self) -> Vec<Sighting> {
        self.0.lock().expect("an unpoisoned record").clone()
    }
}

impl Observer<App> for Sightings {
    fn on_request(&self, request: &Request, route: Option<Route<'_>>, _: &App) {
        let route = route.map(|route| {
            (
                route.method(),
                route.path().to_owned(),
                route.operation_id().to_owned(),
            )
        });
        self.0
            .lock()
            .expect("an unpoisoned record")
            .push((request.method().clone(), route));
    }

    fn on_response(&self, _: &Response, _: Option<Route<'_>>, _: Duration) {}
}

/// A derived HEAD runs the GET operation, so an observer is told that is the
/// operation it ran -- while the request it is handed still says `HEAD`.
#[tokio::test]
async fn a_derived_head_is_reported_as_the_get_operation() {
    let sightings = Sightings::default();
    let service = support::router()
        .observe(sightings.clone())
        .build(App::new())
        .expect("a describable router");

    get(&service, "/users/42").call().await;
    send(&service, Method::HEAD, "/users/42").call().await;

    let [(get_method, get_route), (head_method, head_route)] =
        <[Sighting; 2]>::try_from(sightings.taken()).expect("one sighting per request");

    assert_eq!((get_method, head_method), (Method::GET, Method::HEAD));
    let get_route = get_route.expect("the GET was routed");
    assert_eq!(get_route.0, openapi::Method::Get);
    assert_eq!(head_route, Some(get_route));
}

/// A declared `head` whose handler returns a body anyway.
#[kynos::head("/loud")]
async fn loud_head() -> Text {
    Text("loud".to_owned())
}

/// RFC 9110 section 9.3.2's "MUST NOT send content" is about the method, not
/// about who answered it: a declared `head` returning a body, a 404 problem and
/// a 400 rejection all reach the peer with none. HTTP/1.1 would drop it on the
/// wire; HTTP/2 sends what it is handed.
#[tokio::test]
async fn no_response_to_a_head_carries_content() {
    let service = support::router()
        .mount(kynos::routes![loud_head])
        .build(App::new())
        .expect("a describable router");

    for (target, status) in [
        ("/loud", StatusCode::OK),
        ("/widgets", StatusCode::NOT_FOUND),
        ("/users/nope", StatusCode::BAD_REQUEST),
    ] {
        let reply = send(&service, Method::HEAD, target).call().await;

        assert_eq!(reply.status, status, "{target}");
        assert!(reply.body.is_empty(), "{target}: {:?}", reply.text());
    }

    let loud = send(&service, Method::HEAD, "/loud").call().await;
    assert_eq!(
        loud.field(header::CONTENT_LENGTH.as_str()).as_deref(),
        Some("4")
    );
}

/// A path whose `GET` has content and whose declared `head` has none.
#[kynos::get("/quiet")]
async fn quiet_get() -> Text {
    Text("hello world".to_owned())
}

#[kynos::head("/quiet")]
async fn quiet_head() {}

/// A `GET` whose representation is empty.
#[kynos::get("/empty")]
async fn empty_get() -> Text {
    Text(String::new())
}

/// RFC 9110 section 8.6: a server "MUST NOT send Content-Length" on a HEAD
/// "unless its field value equals the decimal number of octets that would have
/// been sent" to a GET. A handler answering a declared `head` with no body is
/// not stating that its GET is empty, so a zero is never stated -- the rule
/// hyper's HTTP/1.1 encoder already keeps, and one a derived HEAD of an empty
/// GET follows too.
#[tokio::test]
async fn a_head_states_no_length_of_zero() {
    let service = support::router()
        .mount(kynos::routes![quiet_get, quiet_head, empty_get])
        .build(App::new())
        .expect("a describable router");

    for target in ["/quiet", "/empty"] {
        let reply = send(&service, Method::HEAD, target).call().await;

        assert_eq!(reply.status, StatusCode::OK, "{target}");
        assert_eq!(
            reply.field(header::CONTENT_LENGTH.as_str()),
            None,
            "{target}"
        );
    }
}

/// A path declaring `post` and nothing else.
#[kynos::post("/submissions")]
async fn submit() -> NoContent {
    NoContent
}

/// A HEAD refused on a matched path loses its content like any other: the
/// service implements HEAD through `GET /users`, so a HEAD to a POST-only path
/// is a 405, and its problem body would reach an HTTP/2 peer if it were kept.
#[tokio::test]
async fn a_head_refused_on_a_matched_path_carries_no_content() {
    let service = support::router()
        .mount(kynos::routes![submit])
        .build(App::new())
        .expect("a describable router");

    let reply = send(&service, Method::HEAD, "/submissions").call().await;

    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reply.field(header::ALLOW.as_str()).as_deref(), Some("POST"));
    assert!(reply.body.is_empty(), "{:?}", reply.text());
}

/// A path declaring both `get` and `head` names HEAD once in a 405's `Allow`:
/// the declared one, and no second one derived from the GET.
#[tokio::test]
async fn a_declared_head_is_named_once_in_allow() {
    let service = support::router()
        .mount(kynos::routes![both_get, both_head])
        .build(App::new())
        .expect("a describable router");

    // `POST` is implemented on `/users`, so this is a 405 rather than a 501.
    let reply = send(&service, Method::POST, "/both").call().await;

    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        reply.field(header::ALLOW.as_str()).as_deref(),
        Some("GET, HEAD")
    );
}

/// A `tower` layer wraps the operation from outside, and its response still
/// leaves the dispatcher as a HEAD's: no content, and the GET's length.
#[cfg(feature = "unchecked")]
#[tokio::test]
async fn a_head_through_an_unchecked_layer_carries_no_content() {
    let service = support::router()
        .layer_unchecked(tower::layer::util::Identity::new())
        .build(App::new())
        .expect("a describable router");

    let full = get(&service, "/users/42").call().await;
    let head = send(&service, Method::HEAD, "/users/42").call().await;

    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty(), "{:?}", head.text());
    assert_eq!(
        head.field(header::CONTENT_LENGTH.as_str()),
        Some(full.body.len().to_string())
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
async fn a_flipped_spelling_still_refuses_a_method_the_path_does_not_declare() {
    let service = support::router()
        .trailing_slashes(TrailingSlashPolicy::Lenient)
        .build(App::new())
        .expect("a describable router");

    let reply = send(&service, Method::POST, "/users/42/").call().await;

    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);

    // Sorted before comparing, as the declared spelling's own 405 test does:
    // `Allow` reports a set, and its order is the order operations were
    // mounted rather than anything a client may rely on.
    let allow = reply.field(header::ALLOW.as_str()).expect("an Allow field");
    let mut methods: Vec<&str> = allow.split(", ").collect();
    methods.sort_unstable();

    assert_eq!(methods, ["DELETE", "GET", "HEAD"]);
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

    let refused = send(&service, Method::POST, "/users/42").call().await;
    assert_eq!(refused.status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(refused.body.is_empty(), "{:?}", refused.text());

    // The status is not the policy's to choose, so `Allow` survives the shape.
    assert!(refused.field(header::ALLOW.as_str()).is_some());

    // The 501 takes the method-not-allowed policy's shape: both refuse a
    // method, and a second policy would be a second answer to one question.
    let unimplemented = send(&service, Method::PATCH, "/users/42").call().await;
    assert_eq!(unimplemented.status, StatusCode::NOT_IMPLEMENTED);
    assert!(unimplemented.body.is_empty(), "{:?}", unimplemented.text());
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

// --- A literal beside a variable sibling ----------------------------------

/// What `/members/{name}` captures: text, so any segment a literal does not
/// claim is one the variable can take.
#[derive(kynos::Schema, kynos::PathParams)]
struct MemberPath {
    name: String,
}

/// The literal sibling.
#[kynos::get("/members/me")]
async fn member_me() -> Text {
    Text("literal".to_owned())
}

/// The variable sibling, echoing what it captured.
#[kynos::get("/members/{name}")]
async fn member_named(Path(path): Path<MemberPath>) -> Text {
    Text(format!("variable {}", path.name))
}

/// A literal one segment deeper, under the literal `me`.
#[kynos::get("/members/me/posts")]
async fn member_me_posts() -> Text {
    Text("literal posts".to_owned())
}

/// A deeper path the literal `me` has no sibling for.
#[kynos::get("/members/{name}/friends")]
async fn member_friends(Path(path): Path<MemberPath>) -> Text {
    Text(format!("variable {} friends", path.name))
}

/// The four sibling operations, built.
fn members() -> kynos::router::service::Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![
            member_me,
            member_named,
            member_me_posts,
            member_friends
        ])
        .build(())
        .expect("a describable router")
}

/// `docs/routing.md`'s `matchit` contract: a literal segment and a variable
/// may be siblings, and the literal wins the segment it spells. Mount order is
/// not what decides it -- `members` mounts the literal first, and the second
/// router mounts the variable first.
#[tokio::test]
async fn a_literal_segment_wins_over_a_variable_sibling() {
    let service = members();

    assert_eq!(get(&service, "/members/me").call().await.text(), "literal");
    assert_eq!(
        get(&service, "/members/ada").call().await.text(),
        "variable ada"
    );

    let reversed = Router::<()>::new()
        .mount(kynos::routes![member_named, member_me])
        .build(())
        .expect("a describable router");
    assert_eq!(get(&reversed, "/members/me").call().await.text(), "literal");
}

/// A request entering the literal `me` and finding no continuation there falls
/// back to the variable sibling, which captures `me` -- the backtracking the
/// allocation caveat in `docs/routing.md` is written about.
#[tokio::test]
async fn a_request_backtracks_from_a_literal_into_a_variable_sibling() {
    let service = members();

    assert_eq!(
        get(&service, "/members/me/posts").call().await.text(),
        "literal posts"
    );
    assert_eq!(
        get(&service, "/members/me/friends").call().await.text(),
        "variable me friends"
    );
}

/// A `{param}` matches exactly one segment: two segments are no match for one
/// variable, and an encoded slash inside a segment is part of that segment,
/// handed to the handler decoded.
#[tokio::test]
async fn a_variable_never_spans_a_slash() {
    let service = members();

    assert_eq!(
        get(&service, "/members/1/2").call().await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&service, "/members/a%2Fb").call().await.text(),
        "variable a/b"
    );
}

// --- `Forwarded` as an argument -------------------------------------------

/// The client the router resolved, or `none` when it resolved none.
#[kynos::get("/origin")]
async fn origin(forwarded: Forwarded) -> Text {
    Text(
        forwarded
            .client()
            .map_or_else(|| "none".to_owned(), |client| client.to_string()),
    )
}

/// A handler reads the origin the router resolved, under the router's trust
/// policy and nobody else's.
///
/// The same `Forwarded` field is sent to both services. Trusting one hop
/// believes it; trusting none leaves the socket peer, which a driven service
/// has none of. A handler that could see the field without the policy would
/// answer the address both times.
#[tokio::test]
async fn a_handler_reads_the_origin_the_router_resolved() {
    for (trusted, expected) in [
        (TrustedProxies::hops(1), "203.0.113.7"),
        (TrustedProxies::none(), "none"),
    ] {
        let service = Router::<()>::new()
            .mount(kynos::routes![origin])
            .trusted_proxies(trusted)
            .build(())
            .expect("a describable router");

        let answered = get(&service, "/origin")
            .header("forwarded", "for=203.0.113.7")
            .call()
            .await;
        assert_eq!(answered.text(), expected);
    }
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
