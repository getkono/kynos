//! Assembling a router from parts, and where a tag comes from.
//!
//! Run it without the JSON codec, since nothing here has a body:
//!
//! ```text
//! cargo run -p kynos --example composition --no-default-features \
//!   --features openapi31,macros,server,http1
//! ```
//!
//! Four things are worth noticing:
//!
//! * **Scope in the document matches scope in the router, exactly.** A group's
//!   prefix joins each path beneath it, its tag lands on each operation, and
//!   its interceptors contribute to each description. Nobody maintains that by
//!   hand, which is why attaching authentication to a group documents it
//!   correctly on every operation underneath.
//! * **A tag is a type.** `#[derive(Tag)]` on a unit struct, so a misspelling
//!   is a compile error rather than a second tag in the emitted document. It
//!   can be applied at four scopes, and they add rather than override.
//! * **`nest` and `merge` differ in whether a prefix is introduced.** `nest`
//!   puts a whole router under a path; `merge` unions two operation sets at the
//!   same level. Neither can produce a path the description cannot express.
//! * **The panic policy is in the type.** `catch_panics` returns a differently
//!   parameterised `Router`, so whether an operation has a recovery boundary is
//!   decided at compile time and its 500 is contributed for the same reason.
//!
//! `EndpointBuilder` is the escape hatch for a route set that is not known at
//! compile time. It is deliberately weaker: it cannot check that a handler's
//! path parameters match its path template, because at that point the template
//! is a value rather than a literal.

use std::net::Ipv4Addr;

use kynos::{
    openapi::{Method, PathTemplate},
    prelude::*,
    router::{
        endpoint::builder::EndpointBuilder,
        policy::{FallbackPolicy, TrailingSlashPolicy},
    },
    server::Server,
};

/// Everything a consumer does with their own account.
///
/// With no `name`, a tag is its own identifier — so there is no string to
/// misspell in the first place.
#[derive(Tag)]
struct Users;

/// Operational endpoints, which a consumer never calls.
#[derive(Tag)]
#[tag(name = "ops", description = "Health and readiness")]
struct Ops;

/// Everything an administrator does.
#[derive(Tag)]
#[tag(name = "admin", description = "Restricted to staff")]
struct Admin;

/// Reports liveness.
#[kynos::get("/live", tag = Ops)]
async fn live() -> NoContent {
    NoContent
}

/// Reports readiness.
///
/// The fourth tag scope: written on the attribute, so it is a fact about the
/// operation rather than about what encloses it — and the only one readable
/// without building a router, as `EndpointMeta::TAGS`.
#[kynos::get("/ready", tag = Ops)]
async fn ready() -> NoContent {
    NoContent
}

/// Lists users.
///
/// Untagged here: the group it is mounted in supplies `Users`, which is the
/// scope the tag actually belongs to.
#[kynos::get("/", catch_panics)]
async fn list_users() -> NoContent {
    NoContent
}

/// Fetches one user.
#[kynos::get("/{id}")]
async fn get_user(Path(path): Path<UserPath>) -> NoContent {
    let _ = path;
    todo!("the router is still a skeleton; this example exists to typecheck")
}

/// What `/{id}` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct UserPath {
    id: u64,
}

/// Suspends a user.
///
/// `operation` is the attribute for a method with no dedicated one. `PATCH` is
/// one of the eight OpenAPI 3.1 names, so it needs nothing further; a method
/// outside them — `PROPFIND`, `LOCK` — emits into `additionalOperations` and
/// therefore requires the `openapi32` feature.
#[kynos::operation(method = "PATCH", path = "/{id}/suspension", tag = Admin)]
async fn suspend_user(Path(path): Path<UserPath>) -> NoContent {
    let _ = path;
    todo!("the router is still a skeleton; this example exists to typecheck")
}

/// A router that knows nothing about where it will be mounted.
///
/// This is what makes a subsystem reusable: it declares its own paths relative
/// to nothing, and `nest` decides where they land.
fn admin_router() -> Router<()> {
    Router::<()>::new()
        .tag::<Admin>()
        .mount(kynos::routes![suspend_user])
}

/// A router assembled from a builder rather than from attributes.
///
/// Reach for this only when the route set is not known at compile time.
fn dynamic_router() -> Router<()> {
    let endpoint = EndpointBuilder::new(
        Method::Get,
        PathTemplate::parse("/version").expect("valid path"),
        version,
    )
    .operation_id("getVersion")
    .summary("The running build")
    .tag::<Ops>()
    .deprecated();

    Router::<()>::new().mount(endpoint)
}

/// Reports the running build.
async fn version() -> NoContent {
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new()
        // The first tag scope: everything in this router, whatever else it
        // also carries.
        .tag::<Ops>()
        // A group is the recommended unit of API structure, one per resource.
        // The second tag scope, and the prefix that joins each path beneath.
        .group(
            Group::new("/users")
                .tag::<Users>()
                .mount(kynos::routes![list_users, get_user]),
        )
        // `nest` introduces a prefix; the nested router did not know it.
        .nest("/admin", admin_router())
        // `merge` does not; the two operation sets union at this level.
        .merge(dynamic_router())
        .mount(kynos::routes![live, ready])
        // A problem document rather than an empty body, so a client sees why.
        .not_found(FallbackPolicy::Problem)
        .method_not_allowed(FallbackPolicy::Problem)
        // 308 rather than 301, so the method and body survive the replay.
        .trailing_slashes(TrailingSlashPolicy::Redirect);

    // Worth an integration test of its own: this catches the mistakes that only
    // show up across a whole API, such as a duplicated `operationId` or two
    // paths differing only in variable name.
    for violation in router.validate()? {
        println!("{violation}");
    }

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
