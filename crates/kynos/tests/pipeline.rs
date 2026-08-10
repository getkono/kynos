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
    router::endpoint::{Endpoints, IntoEndpoints},
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

#[test]
fn routes_collects_every_operation() {
    compile_only(|| {
        let endpoints: Endpoints<App> = kynos::routes![health, get_user, create_user];
        assert_eq!(endpoints.len(), 3);
        assert!(!endpoints.is_empty());
    });
}

/// A tuple, an array and a vector are all one `mount` argument, which is what
/// lets a router be assembled from several `routes!` calls.
#[test]
fn endpoint_collections_compose() {
    compile_only(|| {
        let mut sink = Endpoints::<App>::new();
        (
            kynos::routes![health],
            kynos::routes![get_user, create_user],
        )
            .into_endpoints(&mut sink);
        assert_eq!(sink.len(), 3);

        let mut sink = Endpoints::<App>::new();
        [kynos::routes![health], kynos::routes![get_user]].into_endpoints(&mut sink);
        assert_eq!(sink.len(), 2);

        let mut sink = Endpoints::<App>::new();
        vec![kynos::routes![health]].into_endpoints(&mut sink);
        assert_eq!(sink.len(), 1);
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
