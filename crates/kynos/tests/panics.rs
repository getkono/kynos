//! Recovery happens where it was asked for, and nowhere else.
//!
//! One reason: `catch_panics` is a *policy* rather than a behaviour, and a
//! policy is only worth what its negative is worth. Recovering everywhere would
//! pass every positive case here and would silently turn a bug into a 500 in
//! services that never asked for that.
//!
//! Three scopes ask for it — the router, a group and one endpoint — and each is
//! covered with the control that differs in exactly that.

#![cfg(all(feature = "macros", feature = "json"))]

use std::{
    any::Any,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::Duration,
};

use kynos::{
    Router,
    http::{Request, Response, StatusCode},
    middleware::Observer,
    openapi::{Method as OpenApiMethod, PathTemplate},
    response::status::NoContent,
    router::{
        endpoint::builder::EndpointBuilder, group::Group, operation::Route, service::Service,
    },
};

#[path = "support/mod.rs"]
mod support;

use support::get;

/// The operation under test. Nothing else in the fixture panics, so a 500 here
/// can only have come from this.
#[kynos::get("/boom")]
async fn boom() -> NoContent {
    panic!("the handler failed");
}

/// The same operation again under its own name.
///
/// Two are needed rather than one mounted twice, because an `operationId` is
/// unique across the description and the router refuses a repeat — which is
/// itself the guarantee `routing.md` claims.
#[kynos::get("/boom")]
async fn also_boom() -> NoContent {
    panic!("the handler failed");
}

/// One that does not, mounted alongside, so a recovery policy that answered 500
/// for everything would be visible.
#[kynos::get("/fine")]
async fn fine() -> NoContent {
    NoContent
}

/// Runs one request and says whether it came back or unwound.
///
/// The panic hook is silenced for the duration: a recovered panic still prints
/// its message, and a suite that prints a backtrace on a passing test teaches a
/// reader to ignore backtraces.
fn outcome(service: &Service<()>, path: &'static str) -> Result<StatusCode, ()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
        runtime.block_on(async { get(service, path).call().await.status })
    }));
    std::panic::set_hook(hook);

    caught.map_err(|_| ())
}

/// Router scope: every operation under it recovers.
#[test]
fn a_router_that_asked_for_recovery_answers_a_panic_with_a_500() {
    let service = Router::<()>::new()
        .catch_panics()
        .mount(kynos::routes![boom, fine])
        .build(())
        .expect("a describable router");

    assert_eq!(
        outcome(&service, "/boom"),
        Ok(StatusCode::INTERNAL_SERVER_ERROR)
    );
    assert_eq!(outcome(&service, "/fine"), Ok(StatusCode::NO_CONTENT));
}

/// The control. Same two operations, differing in exactly whether recovery was
/// asked for — and the panic reaches the caller.
#[test]
fn a_router_that_did_not_ask_for_recovery_lets_a_panic_through() {
    let service = Router::<()>::new()
        .mount(kynos::routes![boom, fine])
        .build(())
        .expect("a describable router");

    assert_eq!(
        outcome(&service, "/boom"),
        Err(()),
        "a panic was recovered by something that was never asked to catch one"
    );
    assert_eq!(outcome(&service, "/fine"), Ok(StatusCode::NO_CONTENT));
}

/// Group scope: recovery covers what the group encloses and stops there.
#[test]
fn recovery_asked_for_on_a_group_covers_that_group_alone() {
    let service = Router::<()>::new()
        .group(
            Group::<()>::new("/guarded")
                .catch_panics()
                .mount(kynos::routes![boom]),
        )
        .group(Group::<()>::new("/bare").mount(kynos::routes![also_boom]))
        .build(())
        .expect("a describable router");

    assert_eq!(
        outcome(&service, "/guarded/boom"),
        Ok(StatusCode::INTERNAL_SERVER_ERROR)
    );
    assert_eq!(outcome(&service, "/bare/boom"), Err(()));
}

/// The third scope, reached without a route attribute.
async fn guarded_endpoint() -> NoContent {
    panic!("the handler failed");
}

/// Its control.
async fn bare_endpoint() -> NoContent {
    panic!("the handler failed");
}

/// Endpoint scope: the innermost of the three, and the only one whose recovery
/// covers exactly one operation.
#[test]
fn recovery_asked_for_on_one_endpoint_covers_that_endpoint_alone() {
    let at = |path: &str| PathTemplate::parse(path).expect("a usable path template");

    let service = Router::<()>::new()
        .mount((
            EndpointBuilder::<(), _, _>::new(OpenApiMethod::Get, at("/guarded"), guarded_endpoint)
                .catch_panics(),
            EndpointBuilder::<(), _, _>::new(OpenApiMethod::Get, at("/bare"), bare_endpoint),
        ))
        .build(())
        .expect("a describable router");

    assert_eq!(
        outcome(&service, "/guarded"),
        Ok(StatusCode::INTERNAL_SERVER_ERROR)
    );
    assert_eq!(outcome(&service, "/bare"), Err(()));
}

/// Records the path of every operation a panic was reported against.
///
/// Held per test rather than in a `static`, so no two tests can hear each
/// other's panics.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<String>>>);

impl Heard {
    fn paths(&self) -> Vec<String> {
        self.0.lock().expect("an unpoisoned record").clone()
    }
}

impl Observer<()> for Heard {
    fn on_request(&self, _: &Request, _: Option<Route<'_>>, (): &()) {}

    fn on_response(&self, _: &Response, _: Option<Route<'_>>, _: Duration) {}

    fn on_panic(&self, _: &(dyn Any + Send), route: Option<Route<'_>>) {
        let path = route.map_or_else(|| "<unmatched>".to_owned(), |route| route.path().to_owned());
        self.0.lock().expect("an unpoisoned record").push(path);
    }
}

/// A recovered panic is reported whichever scope recovered it: the scopes
/// differ in what they cover, not in whether an observer hears about it.
#[test]
fn a_panic_recovered_at_any_scope_is_reported_to_observers() {
    let at = |path: &str| PathTemplate::parse(path).expect("a usable path template");

    let router = Heard::default();
    let router_scope = Router::<()>::new()
        .catch_panics()
        .mount(kynos::routes![boom])
        .observe(router.clone())
        .build(())
        .expect("a describable router");

    let group = Heard::default();
    let group_scope = Router::<()>::new()
        .group(
            Group::<()>::new("/guarded")
                .catch_panics()
                .mount(kynos::routes![boom]),
        )
        .observe(group.clone())
        .build(())
        .expect("a describable router");

    let endpoint = Heard::default();
    let endpoint_scope = Router::<()>::new()
        .mount(
            EndpointBuilder::<(), _, _>::new(OpenApiMethod::Get, at("/guarded"), guarded_endpoint)
                .catch_panics(),
        )
        .observe(endpoint.clone())
        .build(())
        .expect("a describable router");

    for (scope, service, path, heard) in [
        ("router", &router_scope, "/boom", &router),
        ("group", &group_scope, "/guarded/boom", &group),
        ("endpoint", &endpoint_scope, "/guarded", &endpoint),
    ] {
        assert_eq!(
            outcome(service, path),
            Ok(StatusCode::INTERNAL_SERVER_ERROR),
            "{scope} scope"
        );
        assert_eq!(
            heard.paths(),
            [path],
            "a panic recovered at {scope} scope was not reported exactly once"
        );
    }
}

/// Two scopes asked, one caught: the innermost recovery is the only one that
/// saw the unwind, so the panic is one event, not two.
#[test]
fn a_panic_recovered_at_two_scopes_is_reported_once() {
    let heard = Heard::default();
    let service = Router::<()>::new()
        .catch_panics()
        .mount(
            EndpointBuilder::<(), _, _>::new(
                OpenApiMethod::Get,
                PathTemplate::parse("/guarded").expect("a usable path template"),
                guarded_endpoint,
            )
            .catch_panics(),
        )
        .observe(heard.clone())
        .build(())
        .expect("a describable router");

    assert_eq!(
        outcome(&service, "/guarded"),
        Ok(StatusCode::INTERNAL_SERVER_ERROR)
    );
    assert_eq!(heard.paths(), ["/guarded"]);
}

/// A recovered operation declares the 500 it can now produce.
///
/// The whole design says a response a service can send is a response the
/// document names, and a recovery branch is the one place a status appears
/// without any handler returning it.
#[test]
fn a_recovered_operation_declares_the_status_recovery_produces() {
    let guarded = Router::<()>::new()
        .catch_panics()
        .mount(kynos::routes![fine])
        .openapi()
        .expect("a describable router");

    let bare = Router::<()>::new()
        .mount(kynos::routes![fine])
        .openapi()
        .expect("a describable router");

    let declares_500 = |document: &kynos::openapi::Document| {
        document.paths.items["/fine"]
            .get
            .as_ref()
            .expect("a GET operation")
            .responses
            .responses
            .contains_key("500")
    };

    assert!(declares_500(&guarded));
    assert!(
        !declares_500(&bare),
        "an operation with no recovery declared a status only recovery produces"
    );
}
