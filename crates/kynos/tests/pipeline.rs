//! The route attribute, `routes!`, `mount` and `build` typecheck end to end.
//!
//! This is the hole the API-skeleton milestone existed to close. Before it,
//! `Handler` had no implementations and no way to acquire any, so an
//! attribute-annotated function produced a type that nothing could mount.
//!
//! What it proves is that the *types* line up: that an `async fn` is a
//! `Handler`, that `routes!` collects one, that `Endpoints` accepts what it
//! collects, and that a handler asking for a dependency reaches the context
//! that supplies it. The handlers are given real bodies so the collection
//! assertions can count what actually landed, which is the part a typecheck
//! cannot say.

#![cfg(all(feature = "macros", feature = "json"))]
#![allow(dead_code)]

use kynos::{
    Provider, Schema,
    di::inject::Inject,
    error::rejection::AuthRejection,
    extract::{body::json::Json, params::path::Path},
    handler::Handler,
    response::status::{Created, NoContent},
    router::endpoint::set::{Endpoints, IntoEndpoints},
    security::{Authenticates, Authenticator, auth::Auth, carrier::BearerToken, schemes::Bearer},
};

#[derive(Clone, Debug, PartialEq)]
struct Pool(u32);

#[derive(Provider)]
struct App {
    pool: Pool,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct User {
    id: u64,
    name: String,
}

#[derive(Schema, kynos::PathParams)]
struct UserPath {
    id: u64,
}

/// Nothing in, nothing out.
#[kynos::get("/health")]
async fn health() -> NoContent {
    NoContent
}

/// A head-only handler: every argument reads the request head.
#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>, Inject(pool): Inject<Pool>) -> Json<User> {
    let _ = pool;
    Json(User {
        id: path.id,
        name: "Ada Lovelace".to_owned(),
    })
}

/// A body-consuming handler: the last argument takes the body.
#[kynos::post("/users")]
async fn create_user(Inject(pool): Inject<Pool>, Json(user): Json<User>) -> Created<Json<User>> {
    let _ = pool;
    Created::at(get_user::relative_uri(UserPath { id: user.id }), Json(user))
}

/// A route in a module with no prelude, where `Clone` names another derive:
/// an expansion naming a prelude item unqualified resolves against the user's
/// scope, and here that holds nothing or the wrong thing.
#[no_implicit_prelude]
mod prelude_free {
    #[allow(unused_imports)]
    use ::kynos::Tag as Clone;

    #[::kynos::get("/ping", catch_panics)]
    pub(super) async fn ping() -> ::kynos::response::status::NoContent {
        ::kynos::response::status::NoContent
    }
}

/// It is a handler, and its endpoint the standard library's `Clone` and `Copy`
/// nonetheless.
#[test]
fn a_route_declared_without_a_prelude_derives_the_standard_traits() {
    fn is_standard<T: Clone + Copy + std::fmt::Debug + Default>() {}

    is_handler::<App, _, _>(prelude_free::ping);
    is_standard::<prelude_free::ping>();
}

fn is_handler<C, A, H: Handler<C, A>>(_: H) {}

#[test]
fn an_async_fn_is_a_handler() {
    is_handler::<App, _, _>(health);
    is_handler::<App, _, _>(get_user);
    is_handler::<App, _, _>(create_user);
}

/// `routes!` yields a tuple, not an already-erased collection.
///
/// That is what keeps each operation's own interceptors visible at the mount
/// site: an `Endpoints` cannot say what its members carry, so building one here
/// would erase the stacks `Router::mount` has to check.
#[test]
fn routes_collects_every_operation() {
    let mut sink = Endpoints::<App>::new();
    kynos::routes![health, get_user, create_user].into_endpoints(&mut sink);

    assert_eq!(sink.len(), 3);
}

/// A tuple, an array and a vector are all one `mount` argument, which is what
/// lets a router be assembled from several `routes!` calls.
///
/// Each form is spelled out because each resolves to a different
/// implementation, and each is counted: an implementation that collected the
/// outer container and dropped what it held would still typecheck.
#[test]
fn endpoint_collections_compose() {
    let mut sink = Endpoints::<App>::new();
    (
        kynos::routes![health],
        kynos::routes![get_user, create_user],
    )
        .into_endpoints(&mut sink);
    assert_eq!(sink.len(), 3);

    // An array or a vector still composes, but only over one element type --
    // and two `routes!` calls naming different handlers are different tuples.
    // Nesting them in a tuple is the general form.
    let mut sink = Endpoints::<App>::new();
    [kynos::routes![health], kynos::routes![health]].into_endpoints(&mut sink);
    assert_eq!(sink.len(), 2);

    let mut sink = Endpoints::<App>::new();
    vec![kynos::routes![health]].into_endpoints(&mut sink);
    assert_eq!(sink.len(), 1);
}

/// An empty set is a real value, and the only thing here that needs no builder.
#[test]
fn an_empty_collection_is_empty() {
    let sink = Endpoints::<App>::new();
    assert_eq!(sink.len(), 0);
    assert!(sink.is_empty());
}

/// Mounting is where the context type becomes concrete, so it is where a
/// handler asking for a dependency the context lacks would fail to compile.
#[test]
fn a_router_accepts_what_routes_produces() {
    fn build() -> kynos::Router<App> {
        kynos::Router::<App>::new().mount(kynos::routes![health, get_user, create_user])
    }
    let _ = build;
}

/// The one tag written on the attribute rather than on a builder.
#[derive(kynos::Tag)]
struct Users;

/// Listed under a tag.
#[kynos::get("/users/tagged", tag = Users)]
async fn tagged_list() -> NoContent {
    NoContent
}

/// A tag on the attribute is a compile-time fact about the operation, so it
/// belongs to the same constant set as the method, the path and the summary.
///
/// The three builder levels — `Router::tag`, `Group::tag` and
/// `EndpointBuilder::tag` — apply a tag to whatever they enclose. This is the
/// fourth, and it is the only one readable *without building a router*, which
/// is all this asserts. What the description does with it goes through
/// `from_meta` and the describe walk, and is pinned in `description.rs`: a
/// constant assertion is what let the attribute's tag reach no document at all
/// for as long as it did, so nothing further is asserted here.
#[test]
fn a_route_tag_reaches_the_endpoint_metadata() {
    use kynos::router::endpoint::meta::EndpointMeta;

    // A handler as well as a carrier of a constant, for the reason
    // `each_route_attribute_writes_its_own_method` gives: a tag written onto
    // something unmountable would pass the assertion below unnoticed.
    is_handler::<App, _, _>(tagged_list);

    assert_eq!(<tagged_list as EndpointMeta>::TAGS.len(), 1);
    assert_eq!(<tagged_list as EndpointMeta>::TAGS[0].name(), "Users");
    assert!(<health as EndpointMeta>::TAGS.is_empty());
}

// --- The arity list ------------------------------------------------------

// `Handler` is implemented by a macro run once per arity, four times each: the
// last argument consumes the body or every argument reads only the head, each
// with and without a guard in front. Sixty-six implementations in all, with the
// two that take nothing but a guard or nothing at all, and
// `an_async_fn_is_a_handler` reaches three of them.
//
// A macro list is not a thing a reader checks by eye, and the arity that
// breaks when one is dropped is the last one. So the bounds are witnessed:
// nothing in between can be missing while both ends are present, because they
// are all written by the same expansion.

/// One argument, reading the head. The first entry in the list.
async fn one_part(a1: Inject<Pool>) -> NoContent {
    let _ = a1;
    NoContent
}

/// One argument, consuming the body.
async fn one_body(body: Json<User>) -> NoContent {
    let _ = body;
    NoContent
}

/// Sixteen arguments, reading the head. The last entry in the list.
#[allow(clippy::too_many_arguments)]
async fn sixteen_parts(
    a1: Inject<Pool>,
    a2: Inject<Pool>,
    a3: Inject<Pool>,
    a4: Inject<Pool>,
    a5: Inject<Pool>,
    a6: Inject<Pool>,
    a7: Inject<Pool>,
    a8: Inject<Pool>,
    a9: Inject<Pool>,
    a10: Inject<Pool>,
    a11: Inject<Pool>,
    a12: Inject<Pool>,
    a13: Inject<Pool>,
    a14: Inject<Pool>,
    a15: Inject<Pool>,
    a16: Inject<Pool>,
) -> NoContent {
    let _ = (
        a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16,
    );
    NoContent
}

/// Sixteen arguments, the last consuming the body.
#[allow(clippy::too_many_arguments)]
async fn sixteen_with_body(
    a1: Inject<Pool>,
    a2: Inject<Pool>,
    a3: Inject<Pool>,
    a4: Inject<Pool>,
    a5: Inject<Pool>,
    a6: Inject<Pool>,
    a7: Inject<Pool>,
    a8: Inject<Pool>,
    a9: Inject<Pool>,
    a10: Inject<Pool>,
    a11: Inject<Pool>,
    a12: Inject<Pool>,
    a13: Inject<Pool>,
    a14: Inject<Pool>,
    a15: Inject<Pool>,
    body: Json<User>,
) -> NoContent {
    let _ = (
        a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, body,
    );
    NoContent
}

/// The one token `Tokens` accepts.
const VALID_TOKEN: &str = "valid";

/// Accepts `VALID_TOKEN` and refuses every other: the witnesses below need only
/// a context that can check one, and the ordering case a token that passes.
struct Tokens;

impl<C: Sync> Authenticator<Bearer, C> for Tokens {
    async fn authenticate(&self, token: BearerToken, _: &C) -> Result<String, AuthRejection> {
        if token.as_str() == VALID_TOKEN {
            Ok(token.into_inner())
        } else {
            Err(AuthRejection::unauthenticated())
        }
    }

    async fn authorize(
        &self,
        _: &String,
        _: &[kynos::security::Unscoped],
        _: &C,
    ) -> Result<(), AuthRejection> {
        Ok(())
    }
}

impl Authenticates<Bearer> for App {
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Tokens {
        &Tokens
    }
}

/// A guard and nothing else. Its own implementation, beside the empty one.
async fn guard_only(caller: Auth<Bearer>) -> NoContent {
    let _ = caller;
    NoContent
}

/// A guard, then one argument reading the head.
async fn guard_one_part(caller: Auth<Bearer>, a1: Inject<Pool>) -> NoContent {
    let _ = (caller, a1);
    NoContent
}

/// A guard, then one argument consuming the body.
async fn guard_one_body(caller: Auth<Bearer>, body: Json<User>) -> NoContent {
    let _ = (caller, body);
    NoContent
}

/// A guard, then sixteen arguments reading the head.
#[allow(clippy::too_many_arguments)]
async fn guard_sixteen_parts(
    caller: Auth<Bearer>,
    a1: Inject<Pool>,
    a2: Inject<Pool>,
    a3: Inject<Pool>,
    a4: Inject<Pool>,
    a5: Inject<Pool>,
    a6: Inject<Pool>,
    a7: Inject<Pool>,
    a8: Inject<Pool>,
    a9: Inject<Pool>,
    a10: Inject<Pool>,
    a11: Inject<Pool>,
    a12: Inject<Pool>,
    a13: Inject<Pool>,
    a14: Inject<Pool>,
    a15: Inject<Pool>,
    a16: Inject<Pool>,
) -> NoContent {
    let _ = (
        caller, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16,
    );
    NoContent
}

/// A guard, then sixteen arguments, the last consuming the body.
#[allow(clippy::too_many_arguments)]
async fn guard_sixteen_with_body(
    caller: Auth<Bearer>,
    a1: Inject<Pool>,
    a2: Inject<Pool>,
    a3: Inject<Pool>,
    a4: Inject<Pool>,
    a5: Inject<Pool>,
    a6: Inject<Pool>,
    a7: Inject<Pool>,
    a8: Inject<Pool>,
    a9: Inject<Pool>,
    a10: Inject<Pool>,
    a11: Inject<Pool>,
    a12: Inject<Pool>,
    a13: Inject<Pool>,
    a14: Inject<Pool>,
    a15: Inject<Pool>,
    body: Json<User>,
) -> NoContent {
    let _ = (
        caller, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, body,
    );
    NoContent
}

#[test]
fn both_ends_of_the_arity_list_are_handlers() {
    // Zero takes neither marker: there is nothing to tell apart.
    is_handler::<App, _, _>(health);

    is_handler::<App, _, _>(one_part);
    is_handler::<App, _, _>(one_body);
    is_handler::<App, _, _>(sixteen_parts);
    is_handler::<App, _, _>(sixteen_with_body);

    // The guard's slot is in addition to the sixteen, at both ends.
    is_handler::<App, _, _>(guard_only);
    is_handler::<App, _, _>(guard_one_part);
    is_handler::<App, _, _>(guard_one_body);
    is_handler::<App, _, _>(guard_sixteen_parts);
    is_handler::<App, _, _>(guard_sixteen_with_body);
}

/// A guard, then the body, mounted so a request reaches it.
#[kynos::post("/guarded")]
async fn guarded_create(caller: Auth<Bearer>, Json(user): Json<User>) -> NoContent {
    let _ = (caller, user);
    NoContent
}

/// Sends a malformed JSON body to `guarded_create`, with `authorization` when
/// given one, through `Service::call` for the reason `cors.rs`'s `send` gives.
async fn send_malformed(authorization: Option<&str>) -> kynos::http::Response {
    use kynos::http::{HeaderValue, Method, Request, header};

    let service = kynos::Router::<App>::new()
        .mount(kynos::routes![guarded_create])
        .build(App { pool: Pool(1) })
        .expect("a describable router");

    let mut request = Request::new(kynos::http::body::Body::from("{"));
    *request.method_mut() = Method::POST;
    *request.uri_mut() = "/guarded".parse().expect("a usable path");
    request.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    if let Some(value) = authorization {
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(value).expect("a usable field value"),
        );
    }

    service.call(request).await
}

/// The guard runs before every other argument, so a request both
/// unauthenticated and malformed is refused for its credential: 401 with the
/// challenge RFC 9110 section 11.6.1 requires, never the 400 the body earns,
/// which would tell a caller with no credential how the operation parses.
///
/// The control sends the same body under a token the guard accepts, and is
/// refused for the body, so the 401 is the guard's and not a refusal of
/// everything.
#[tokio::test]
async fn the_guard_refuses_before_the_body_is_read() {
    use kynos::http::{StatusCode, header};

    let refused = send_malformed(None).await;
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        refused
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .map(|value| value.to_str().expect("a visible challenge")),
        Some("Bearer"),
    );

    let control = send_malformed(Some(&format!("Bearer {VALID_TOKEN}"))).await;
    assert_eq!(control.status(), StatusCode::BAD_REQUEST);
    assert_eq!(control.headers().get(header::WWW_AUTHENTICATE), None);
}

/// The witnessed top arity, counted against the list that produces it.
///
/// Witnessing both ends says nothing about where the list ends, so this is what
/// notices the list growing: extend it to twenty and the top witness above is
/// no longer the top.
#[test]
fn the_arity_list_ends_where_its_witness_does() {
    const SOURCE: &str = include_str!("../src/handler/impls.rs");
    const WITNESSED: usize = 16;

    let arities = SOURCE.matches("impl_handler!(").count();
    assert_eq!(
        arities, WITNESSED,
        "`impls.rs` implements {arities} arities and the top one witnessed is {WITNESSED}; \
         an arity added without a witness is one nothing asks to typecheck"
    );
}

// --- The route attributes ------------------------------------------------

// Nine attributes write a method into `EndpointMeta::METHOD`, and two of them
// were exercised. `patch`, `head`, `options` and `trace` appeared in no test
// and no example anywhere in the workspace, and `put` and `delete` only in
// examples, which are built rather than asserted. Six of the nine could have
// written any method at all.

#[kynos::put("/things")]
async fn put_thing() -> NoContent {
    NoContent
}

#[kynos::patch("/things")]
async fn patch_thing() -> NoContent {
    NoContent
}

#[kynos::delete("/things")]
async fn delete_thing() -> NoContent {
    NoContent
}

#[kynos::head("/things")]
async fn head_thing() -> NoContent {
    NoContent
}

#[kynos::options("/things")]
async fn options_thing() -> NoContent {
    NoContent
}

#[kynos::trace("/things")]
async fn trace_thing() -> NoContent {
    NoContent
}

/// `QUERY` has no Path Item field before OpenAPI 3.2, so the attribute that
/// writes it is gated the same way.
#[cfg(feature = "openapi32")]
#[kynos::query("/things")]
async fn query_thing() -> NoContent {
    NoContent
}

/// The escape hatch for a method with no attribute of its own, given one that
/// has: what is under test is that it writes what it was told. `PATCH` rather
/// than `GET`, the method its own documentation example and `#[kynos::get]`
/// write, so an expansion that wrote `GET` whatever it was told would fail.
#[kynos::operation(method = "PATCH", path = "/things/described")]
async fn described_thing() -> NoContent {
    NoContent
}

#[test]
fn each_route_attribute_writes_its_own_method() {
    use kynos::router::endpoint::meta::EndpointMeta;

    // Also handlers, not only carriers of a constant: an attribute that wrote
    // the right method onto something unmountable would pass the assertions
    // below and nothing else here would notice.
    is_handler::<App, _, _>(put_thing);
    is_handler::<App, _, _>(patch_thing);
    is_handler::<App, _, _>(delete_thing);
    is_handler::<App, _, _>(head_thing);
    is_handler::<App, _, _>(options_thing);
    is_handler::<App, _, _>(trace_thing);
    #[cfg(feature = "openapi32")]
    is_handler::<App, _, _>(query_thing);
    is_handler::<App, _, _>(described_thing);

    assert_eq!(<health as EndpointMeta>::METHOD, "GET");
    assert_eq!(<create_user as EndpointMeta>::METHOD, "POST");
    assert_eq!(<put_thing as EndpointMeta>::METHOD, "PUT");
    assert_eq!(<patch_thing as EndpointMeta>::METHOD, "PATCH");
    assert_eq!(<delete_thing as EndpointMeta>::METHOD, "DELETE");
    assert_eq!(<head_thing as EndpointMeta>::METHOD, "HEAD");
    assert_eq!(<options_thing as EndpointMeta>::METHOD, "OPTIONS");
    assert_eq!(<trace_thing as EndpointMeta>::METHOD, "TRACE");
    #[cfg(feature = "openapi32")]
    assert_eq!(<query_thing as EndpointMeta>::METHOD, "QUERY");
    assert_eq!(<described_thing as EndpointMeta>::METHOD, "PATCH");
}

// The count that ties the cases above to the attributes `kynos-macros`
// actually declares lives in `ledger.rs`, for the reason stated there: reading
// the sibling crate's source leaves this package, so the target carrying that
// read is excluded from the published archive.
