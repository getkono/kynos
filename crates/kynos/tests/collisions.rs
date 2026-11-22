//! Two interceptors covering one operation, where no type carried both.
//!
//! The collision rule is `CompatibleWith`'s, and a compile error wherever both
//! interceptors' types meet in one scope's list. An operation collected into an
//! `Endpoints` erases what it holds, and a `Group`'s type leaves out what its
//! operations hold, so there the router compares the pair as it is built
//! instead, and refuses the build with the operation's location.
//! A collision is a refusal with no diagnostic text of its own, so it is
//! asserted here rather than by a `trybuild` pair, each beside the control that
//! differs in exactly the property under test.

#![cfg(feature = "macros")]

use std::time::Duration;

use kynos::{
    Error, Router,
    middleware::{
        limits::{body_size::BodySize, timeout::Timeout},
        request_id::RequestId,
    },
    openapi::Violation,
    router::{
        endpoint::set::Endpoints,
        error::{Overlap, RouterError},
        group::{Group, SafeMethods},
    },
};

#[kynos::get("/users")]
async fn list() {}

#[kynos::post("/users")]
async fn create() {}

/// What building `router` refused it with.
fn refusal<P, I, S>(router: Router<(), P, I, S>) -> Vec<Violation<RouterError>>
where
    P: kynos::middleware::catch_panic::PanicPolicy,
{
    match router.build(()) {
        Err(Error::Invalid { violations }) => violations,
        Err(other) => panic!("refused for another reason: {other}"),
        Ok(_) => panic!("built"),
    }
}

/// `list` holding `interceptor`, in a collection whose type says nothing of it.
fn collected<N: kynos::middleware::Interceptor<()>>(interceptor: N) -> Endpoints<()> {
    let mut endpoints = Endpoints::new();
    endpoints.push(kynos::routes![list].0.intercept(interceptor));
    endpoints
}

/// A router's interceptor and one an operation in a collection holds both
/// write `x-request-id`, so a client could not tell which one it read.
#[test]
fn a_collected_operation_is_compared_against_the_router() {
    let violations = refusal(
        Router::<()>::new()
            .intercept(RequestId::new())
            .mount(collected(RequestId::new())),
    );

    let [violation] = violations.as_slice() else {
        panic!("one violation: {violations:#?}");
    };
    let request_id = std::any::type_name::<RequestId>();
    assert_eq!(
        violation.error,
        RouterError::InterceptorsCollide {
            endpoint: request_id,
            scope: request_id,
            overlap: Overlap::Header("x-request-id".to_owned()),
        }
    );
    assert_eq!(
        violation.to_string(),
        format!(
            "error at #/paths/~1users/get: `{request_id}`, which this operation holds, and \
             `{request_id}`, covering it, both add the `x-request-id` response header"
        )
    );

    // The control: a 413 and an `x-request-id` claim nothing in common.
    assert!(
        Router::<()>::new()
            .intercept(RequestId::new())
            .mount(collected(BodySize::new(1024)))
            .build(())
            .is_ok()
    );
}

/// The status half of the rule, through a group: two limits both answering
/// 413.
#[test]
fn a_collected_operation_is_compared_against_its_group() {
    let violations = refusal(
        Router::<()>::new().group(
            Group::<()>::new("/a")
                .intercept(BodySize::new(1024))
                .mount(collected(BodySize::new(64))),
        ),
    );

    let [violation] = violations.as_slice() else {
        panic!("one violation: {violations:#?}");
    };
    assert_eq!(violation.location, "#/paths/~1a~1users/get");
    assert!(matches!(
        violation.error,
        RouterError::InterceptorsCollide {
            overlap: Overlap::Status(413),
            ..
        }
    ));

    // The control: a 408 is not a 413.
    assert!(
        Router::<()>::new()
            .group(
                Group::<()>::new("/a")
                    .intercept(Timeout::new(Duration::from_secs(5)))
                    .mount(collected(BodySize::new(64))),
            )
            .build(())
            .is_ok()
    );
}

/// A group interceptor scoped to methods an operation is not declared with is
/// compared against it all the same, as the compile-time check compares two
/// whose method sets overlap: so mounting a `GET` later cannot turn a group
/// that built into one that does not.
#[test]
fn a_scoped_group_interceptor_is_compared_against_every_operation() {
    let mut writes = Endpoints::new();
    writes.push(kynos::routes![create].0.intercept(RequestId::new()));

    let violations = refusal(
        Router::<()>::new().group(
            Group::<()>::new("/a")
                .intercept_when(SafeMethods, RequestId::new())
                .mount(writes),
        ),
    );

    let [violation] = violations.as_slice() else {
        panic!("one violation: {violations:#?}");
    };
    assert_eq!(violation.location, "#/paths/~1a~1users/post");

    // The control: the same shape over a scoped interceptor claiming nothing
    // the operation's does.
    let mut writes = Endpoints::new();
    writes.push(kynos::routes![create].0.intercept(RequestId::new()));
    assert!(
        Router::<()>::new()
            .group(
                Group::<()>::new("/a")
                    .intercept_when(SafeMethods, BodySize::new(1024))
                    .mount(writes),
            )
            .build(())
            .is_ok()
    );
}

/// `list` holding a `RequestId`, mounted into a group whose type then says
/// nothing of it.
fn holding_a_request_id() -> Group<()> {
    Group::new("/a").mount(kynos::routes![list].0.intercept(RequestId::new()))
}

/// A group interceptor applied after a mount covers what was mounted, so the
/// two are compared, though the group's type no longer carries the first.
/// `catch_panics` between them changes nothing.
#[test]
fn a_group_interceptor_after_a_mount_is_compared_against_it() {
    let refusals = [
        refusal(Router::<()>::new().group(holding_a_request_id().intercept(RequestId::new()))),
        refusal(
            Router::<()>::new().group(
                holding_a_request_id()
                    .catch_panics()
                    .intercept(RequestId::new()),
            ),
        ),
    ];
    for violations in refusals {
        let [violation] = violations.as_slice() else {
            panic!("one violation: {violations:#?}");
        };
        assert_eq!(violation.location, "#/paths/~1a~1users/get");
    }

    // The control: a 408 claims nothing a `RequestId` does.
    assert!(
        Router::<()>::new()
            .group(holding_a_request_id().intercept(Timeout::new(Duration::from_secs(5))))
            .build(())
            .is_ok()
    );
}

/// What a group's operations hold reaches every router above it, whichever
/// order the router's interceptor and the group were written in, and through a
/// nest and a merge.
#[test]
fn a_router_interceptor_is_compared_against_what_a_group_holds() {
    let refusals = [
        refusal(
            Router::<()>::new()
                .intercept(RequestId::new())
                .group(holding_a_request_id()),
        ),
        refusal(
            Router::<()>::new()
                .group(holding_a_request_id())
                .intercept(RequestId::new()),
        ),
        refusal(
            Router::<()>::new()
                .intercept(RequestId::new())
                .nest("/x", Router::<()>::new().group(holding_a_request_id())),
        ),
        refusal(
            Router::<()>::new()
                .intercept(RequestId::new())
                .merge(Router::<()>::new().group(holding_a_request_id())),
        ),
    ];
    let locations: Vec<Vec<String>> = refusals
        .iter()
        .map(|violations| violations.iter().map(|v| v.location.clone()).collect())
        .collect();
    assert_eq!(
        locations,
        [
            ["#/paths/~1a~1users/get"],
            ["#/paths/~1a~1users/get"],
            ["#/paths/~1x~1a~1users/get"],
            ["#/paths/~1a~1users/get"],
        ]
    );

    // The control: two mounts in one group, each holding a `RequestId`, under
    // no interceptor of the group's or a router's. No request reaches both.
    assert!(
        Router::<()>::new()
            .group(
                holding_a_request_id().mount(kynos::routes![create].0.intercept(RequestId::new()))
            )
            .build(())
            .is_ok()
    );
}
