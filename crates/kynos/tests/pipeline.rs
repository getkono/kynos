//! The route attribute, `routes!`, `mount` and `build` typecheck end to end.
//!
//! This is the hole the API-skeleton milestone existed to close. Before it,
//! `Handler` had no implementations and no way to acquire any, so an
//! attribute-annotated function produced a type that nothing could mount.
//!
//! Bodies are still `todo!()`, so nothing here runs a handler. What it proves
//! is that the *types* line up: that an `async fn` is a `Handler`, that
//! `routes!` collects one, that `Endpoints` accepts what it collects, and that
//! a handler asking for a dependency reaches the context that supplies it.

#![cfg(all(feature = "macros", feature = "json"))]
#![allow(dead_code)]

use kynos::{
    Provider, Schema,
    di::inject::Inject,
    extract::{body::json::Json, params::path::Path},
    handler::Handler,
    response::status::{Created, NoContent},
    router::endpoint::set::{Endpoints, IntoEndpoints},
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
    let _ = (path, pool);
    todo!()
}

/// A body-consuming handler: the last argument takes the body.
#[kynos::post("/users")]
async fn create_user(Inject(pool): Inject<Pool>, Json(user): Json<User>) -> Created<Json<User>> {
    let _ = (pool, user);
    todo!()
}

fn is_handler<C, A, H: Handler<C, A>>(_: H) {}

#[test]
fn an_async_fn_is_a_handler() {
    is_handler::<App, _, _>(health);
    is_handler::<App, _, _>(get_user);
    is_handler::<App, _, _>(create_user);
}

/// Anything that builds an endpoint runs behind a branch that is never taken:
/// `EndpointBuilder`'s body is still `todo!()`, so the assertion is that this
/// *typechecks*. `tests/compile/panic_recovery.rs` uses the same guard.
fn compile_only(check: impl FnOnce()) {
    if std::hint::black_box(false) {
        check();
    }
}

/// `routes!` yields a tuple, not an already-erased collection.
///
/// That is what keeps each operation's own interceptors visible at the mount
/// site: an `Endpoints` cannot say what its members carry, so building one here
/// would erase the stacks `Router::mount` has to check.
///
/// Nothing is asserted about what lands in the sink, because nothing here runs:
/// `into_endpoints` reaches `EndpointBuilder`, whose body is `todo!()`. The
/// count belongs with that body when it lands.
#[test]
fn routes_collects_every_operation() {
    compile_only(|| {
        let mut sink = Endpoints::<App>::new();
        kynos::routes![health, get_user, create_user].into_endpoints(&mut sink);
    });
}

/// A tuple, an array and a vector are all one `mount` argument, which is what
/// lets a router be assembled from several `routes!` calls.
///
/// Each form is spelled out because each resolves to a different
/// implementation, and typechecking is the whole of what is claimed here.
#[test]
fn endpoint_collections_compose() {
    compile_only(|| {
        let mut sink = Endpoints::<App>::new();
        (
            kynos::routes![health],
            kynos::routes![get_user, create_user],
        )
            .into_endpoints(&mut sink);

        // An array or a vector still composes, but only over one element
        // type -- and two `routes!` calls naming different handlers are
        // different tuples. Nesting them in a tuple is the general form.
        let mut sink = Endpoints::<App>::new();
        [kynos::routes![health], kynos::routes![health]].into_endpoints(&mut sink);

        let mut sink = Endpoints::<App>::new();
        vec![kynos::routes![health]].into_endpoints(&mut sink);
    });
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
    todo!()
}

/// A tag on the attribute is a compile-time fact about the operation, so it
/// belongs to the same constant set as the method, the path and the summary.
///
/// The three builder levels — `Router::tag`, `Group::tag` and
/// `EndpointBuilder::tag` — apply a tag to whatever they enclose. This is the
/// fourth, and it is the only one the description can read without building a
/// router.
#[test]
fn a_route_tag_reaches_the_endpoint_metadata() {
    use kynos::router::endpoint::meta::EndpointMeta;

    assert_eq!(<tagged_list as EndpointMeta>::TAGS, ["Users"]);
    assert!(<health as EndpointMeta>::TAGS.is_empty());
}

// --- The arity list ------------------------------------------------------

// `Handler` is implemented by a macro run once per arity, twice each: one
// implementation where the last argument consumes the body, one where every
// argument reads only the head. Thirty-three implementations in all, and
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

#[test]
fn both_ends_of_the_arity_list_are_handlers() {
    // Zero takes neither marker: there is nothing to tell apart.
    is_handler::<App, _, _>(health);

    is_handler::<App, _, _>(one_part);
    is_handler::<App, _, _>(one_body);
    is_handler::<App, _, _>(sixteen_parts);
    is_handler::<App, _, _>(sixteen_with_body);
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
/// has: what is under test is that it writes what it was told.
#[kynos::operation(method = "GET", path = "/things/described")]
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
    assert_eq!(<described_thing as EndpointMeta>::METHOD, "GET");
}

/// The attributes, counted against the entry points that declare them.
///
/// Under `openapi32`, because `query` is gated there and the full set only
/// exists in that build -- which is the one `mise run test` uses.
#[cfg(feature = "openapi32")]
#[test]
fn every_route_attribute_has_a_case() {
    const SOURCE: &str = include_str!("../../kynos-macros/src/lib.rs");
    /// The eight ungated attributes, `query`, and `operation`.
    const WITNESSED: usize = 10;

    let declared = SOURCE.matches("#[proc_macro_attribute]").count();
    assert_eq!(
        declared, WITNESSED,
        "`kynos-macros` declares {declared} attribute(s) and {WITNESSED} are witnessed; an \
         attribute added without a case is one whose method nothing reads"
    );
}
