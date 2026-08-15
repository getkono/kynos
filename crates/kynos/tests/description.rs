//! Scope in the router is scope in the document.
//!
//! One reason: `docs/middleware.md` states it as a rule — *"an interceptor
//! mounted on a subtree covers the operations in that subtree and nothing
//! else"* — and nothing in the type system enforces it. A `describe` pass that
//! visited every operation instead of the covered ones would still produce a
//! document that validates, and every other test in the suite would still pass.
//!
//! What is asserted is the declaration rather than the behaviour: the two are
//! the same associated types by construction, and the *behaviour* is already
//! covered by `limits.rs` and `interceptors.rs`. This is about which operations
//! the declaration reaches.

#![cfg(all(feature = "macros", feature = "json"))]

use std::collections::BTreeSet;

use kynos::{
    Router,
    middleware::limits::{BodySize, Timeout},
    openapi::Document,
    response::status::NoContent,
    router::group::Group,
};

#[kynos::get("/alpha")]
async fn alpha() -> NoContent {
    NoContent
}

#[kynos::get("/beta")]
async fn beta() -> NoContent {
    NoContent
}

#[kynos::get("/gamma")]
async fn gamma() -> NoContent {
    NoContent
}

/// Every `paths` key whose operation declares `status`.
fn declaring(document: &Document, status: &str) -> BTreeSet<String> {
    document
        .paths
        .0
        .iter()
        .filter(|(_, item)| {
            item.operations()
                .any(|(_, operation)| operation.responses.responses.contains_key(status))
        })
        .map(|(path, _)| path.clone())
        .collect()
}

fn paths(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// Router scope reaches every operation, including ones mounted afterwards.
///
/// The order matters: an implementation collecting interceptors at `mount`
/// time rather than at `describe` time would cover `alpha` and miss `beta`.
#[test]
fn a_router_scoped_interceptor_declares_on_every_operation_either_side_of_it() {
    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .intercept(BodySize::new(1024))
        .mount(kynos::routes![beta])
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/alpha", "/beta"]));
}

/// Group scope reaches the group and stops there.
#[test]
fn a_group_scoped_interceptor_declares_on_its_group_alone() {
    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .group(
            Group::<()>::new("/inner")
                .intercept(BodySize::new(1024))
                .mount(kynos::routes![beta]),
        )
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/inner/beta"]));
}

/// Endpoint scope reaches one operation.
///
/// The innermost of the three, and the one where an over-broad `describe` pass
/// would be least visible: a document declaring 413 on two operations instead
/// of one still validates.
#[test]
fn an_endpoint_scoped_interceptor_declares_on_its_endpoint_alone() {
    let document = Router::<()>::new()
        .mount((
            kynos::routes![alpha],
            kynos::routes![beta].0.intercept(BodySize::new(1024)),
        ))
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/beta"]));
}

/// Two scopes compose: an operation under both declares both.
#[test]
fn an_operation_under_two_scopes_declares_what_each_contributes() {
    let document = Router::<()>::new()
        .intercept(BodySize::new(1024))
        .mount(kynos::routes![alpha])
        .group(
            Group::<()>::new("/inner")
                .intercept(Timeout::new(std::time::Duration::from_secs(1)))
                .mount(kynos::routes![beta]),
        )
        .mount(kynos::routes![gamma])
        .openapi()
        .expect("a describable router");

    // The router's limit reaches all three.
    assert_eq!(
        declaring(&document, "413"),
        paths(&["/alpha", "/inner/beta", "/gamma"])
    );
    // The group's reaches one, and the one it reaches has both.
    assert_eq!(declaring(&document, "504"), paths(&["/inner/beta"]));
}

/// A nested router carries its own interceptors to exactly what it held.
///
/// `nest` and `merge` are the two ways one router absorbs another, and an
/// absorbed router's interceptors have to become part of what each of *its*
/// operations carries rather than of what the absorbing router applies to all
/// of them.
#[test]
fn a_nested_routers_interceptor_stays_with_what_that_router_held() {
    let inner = Router::<()>::new()
        .intercept(BodySize::new(1024))
        .mount(kynos::routes![beta]);

    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .nest("/v1", inner)
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/v1/beta"]));
}

/// The same, through `merge`, which absorbs without a prefix.
#[test]
fn a_merged_routers_interceptor_stays_with_what_that_router_held() {
    let other = Router::<()>::new()
        .intercept(BodySize::new(1024))
        .mount(kynos::routes![beta]);

    let document = Router::<()>::new()
        .mount(kynos::routes![alpha])
        .merge(other)
        .openapi()
        .expect("a describable router");

    assert_eq!(declaring(&document, "413"), paths(&["/beta"]));
}

/// The control for all of the above: with no interceptor anywhere, no operation
/// declares the status. Without it every assertion here would pass against a
/// `describe` pass that declared nothing at all.
#[test]
fn nothing_declares_a_limits_status_when_no_limit_is_mounted() {
    let document = Router::<()>::new()
        .mount(kynos::routes![alpha, beta, gamma])
        .openapi()
        .expect("a describable router");

    assert!(declaring(&document, "413").is_empty());
    assert!(declaring(&document, "504").is_empty());
}
