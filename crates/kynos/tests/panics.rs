//! Recovery happens where it was asked for, and nowhere else.
//!
//! One reason: `catch_panics` is a *policy* rather than a behaviour, and a
//! policy is only worth what its negative is worth. Recovering everywhere would
//! pass every positive case here and would silently turn a bug into a 500 in
//! services that never asked for that.
//!
//! Three scopes ask for it — the router, a group and one endpoint — and each is
//! covered with the control that differs in exactly that.
//!
//! A recovered panic is also reported to observers once, whichever scope
//! recovered it: the scopes differ in what they cover, not in whether anyone
//! hears about it.

#![cfg(all(feature = "macros", feature = "json"))]

use std::{
    any::Any,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use kynos::{
    Router,
    http::{Request, Response, StatusCode},
    middleware::{Continued, Interceptor, Next, Observer},
    openapi::{Method as OpenApiMethod, PathTemplate},
    response::status::NoContent,
    router::{endpoint::builder::EndpointBuilder, group::Group, operation::Route},
};

#[path = "support/mod.rs"]
mod support;

use support::{caught, outcome};

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

/// The third scope, reached through a route attribute.
#[kynos::get("/attributed", catch_panics)]
async fn attributed() -> NoContent {
    panic!("the handler failed");
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

    let attribute = Heard::default();
    let attribute_scope = Router::<()>::new()
        .mount(kynos::routes![attributed])
        .observe(attribute.clone())
        .build(())
        .expect("a describable router");

    for (scope, service, path, heard) in [
        ("router", &router_scope, "/boom", &router),
        ("group", &group_scope, "/guarded/boom", &group),
        ("endpoint", &endpoint_scope, "/guarded", &endpoint),
        (
            "route attribute",
            &attribute_scope,
            "/attributed",
            &attribute,
        ),
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

/// Records every observer event in the order it arrived, a panic with its
/// payload.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    fn events(&self) -> Vec<String> {
        self.0.lock().expect("an unpoisoned record").clone()
    }

    fn push(&self, event: String) {
        self.0.lock().expect("an unpoisoned record").push(event);
    }
}

impl Observer<()> for Log {
    fn on_request(&self, _: &Request, _: Option<Route<'_>>, (): &()) {
        self.push("request".to_owned());
    }

    fn on_response(&self, response: &Response, _: Option<Route<'_>>, _: Duration) {
        self.push(format!("response {}", response.status().as_u16()));
    }

    fn on_panic(&self, payload: &(dyn Any + Send), _: Option<Route<'_>>) {
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .expect("a panic! with a literal message carries a `&str`");
        self.push(format!("panic {message}"));
    }
}

/// What a recovered panic answers with: a problem document naming the status
/// and nothing else, so the payload — whatever the panicking code put in it —
/// never reaches the client, in the body or in a field.
#[test]
fn a_recovered_panic_answers_a_bare_problem_that_exposes_no_payload() {
    let service = Router::<()>::new()
        .catch_panics()
        .mount(kynos::routes![boom])
        .build(())
        .expect("a describable router");

    let reply = caught(&service, "/boom").expect("a recovered panic");

    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        reply.field("content-type").as_deref(),
        Some("application/problem+json")
    );
    assert_eq!(
        reply.json(),
        serde_json::json!({
            "type": "about:blank",
            "title": "Internal Server Error",
            "status": 500,
        })
    );
    for (name, value) in &reply.headers {
        assert!(
            !String::from_utf8_lossy(value.as_bytes()).contains("the handler failed"),
            "{name} carries the panic's payload"
        );
    }
}

/// An observer is handed the payload the code panicked with, and hears of the
/// panic before it hears of the 500 the panic became — at every scope, the
/// endpoint's included, whose payload travels back to the dispatcher on the
/// response rather than being reported where it was caught.
#[test]
fn an_observer_hears_the_payload_before_the_500_it_became() {
    let at = |path: &str| PathTemplate::parse(path).expect("a usable path template");

    let router = Log::default();
    let router_scope = Router::<()>::new()
        .catch_panics()
        .mount(kynos::routes![boom])
        .observe(router.clone())
        .build(())
        .expect("a describable router");

    let group = Log::default();
    let group_scope = Router::<()>::new()
        .group(
            Group::<()>::new("/guarded")
                .catch_panics()
                .mount(kynos::routes![boom]),
        )
        .observe(group.clone())
        .build(())
        .expect("a describable router");

    let endpoint = Log::default();
    let endpoint_scope = Router::<()>::new()
        .mount(
            EndpointBuilder::<(), _, _>::new(OpenApiMethod::Get, at("/guarded"), guarded_endpoint)
                .catch_panics(),
        )
        .observe(endpoint.clone())
        .build(())
        .expect("a describable router");

    for (scope, service, path, log) in [
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
            log.events(),
            ["request", "panic the handler failed", "response 500"],
            "{scope} scope"
        );
    }
}

/// An interceptor that panics on every request, whatever answers it.
struct Exploding;

impl Interceptor<()> for Exploding {
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        _: Request,
        (): (),
        (): &(),
        _: Next<'_, ()>,
    ) -> Result<Continued<()>, Infallible> {
        panic!("the interceptor failed");
    }
}

/// An interceptor that panics while it is handed the request, before it builds
/// the future it returns.
struct ExplodingEarly;

impl Interceptor<()> for ExplodingEarly {
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    #[expect(unreachable_code, reason = "the panic is the point")]
    fn intercept(
        &self,
        _: Request,
        (): (),
        (): &(),
        _: Next<'_, ()>,
    ) -> impl Future<Output = Result<Continued<()>, Infallible>> + Send {
        panic!("the interceptor failed");
        std::future::pending()
    }
}

/// `Next::run` hands the request over when it is called rather than when it is
/// polled, so an interceptor's work before its future exists runs early; the
/// router's recovery still covers it, and the control still unwinds.
#[test]
fn a_router_recovers_a_panic_raised_before_an_interceptor_built_its_future() {
    let recovering = Router::<()>::new()
        .catch_panics()
        .mount(kynos::routes![fine])
        .intercept(ExplodingEarly)
        .build(())
        .expect("a describable router");
    let propagating = Router::<()>::new()
        .mount(kynos::routes![fine])
        .intercept(ExplodingEarly)
        .build(())
        .expect("a describable router");

    assert_eq!(
        outcome(&recovering, "/fine"),
        Ok(StatusCode::INTERNAL_SERVER_ERROR)
    );
    assert_eq!(outcome(&propagating, "/fine"), Err(()));
}

/// A router's interceptor also runs around a fallback, so the router's
/// recovery covers it there: a 500 with recovery, reported with no route since
/// no operation matched, and an unwind without it.
#[test]
fn a_router_recovers_a_panic_its_interceptor_raised_around_a_fallback() {
    let heard = Heard::default();
    let recovering = Router::<()>::new()
        .catch_panics()
        .mount(kynos::routes![fine])
        .intercept(Exploding)
        .observe(heard.clone())
        .build(())
        .expect("a describable router");
    let propagating = Router::<()>::new()
        .mount(kynos::routes![fine])
        .intercept(Exploding)
        .build(())
        .expect("a describable router");

    assert_eq!(
        outcome(&recovering, "/nowhere"),
        Ok(StatusCode::INTERNAL_SERVER_ERROR)
    );
    assert_eq!(heard.paths(), ["<unmatched>"]);
    assert_eq!(outcome(&propagating, "/nowhere"), Err(()));
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

/// A `tower` layer re-enters the table beneath itself, so the endpoint's 500
/// reaches the dispatcher by that second way in and is reported there.
///
/// `Identity` is enough: what is under test is the re-entry, not anything a
/// layer does to the response.
#[cfg(feature = "unchecked")]
#[test]
fn a_panic_an_endpoint_recovered_beneath_an_unchecked_layer_is_reported_once() {
    let heard = Heard::default();
    let service = Router::<()>::new()
        .layer_unchecked(tower::layer::util::Identity::new())
        .mount(kynos::routes![attributed])
        .observe(heard.clone())
        .build(())
        .expect("a describable router");

    assert_eq!(
        outcome(&service, "/attributed"),
        Ok(StatusCode::INTERNAL_SERVER_ERROR)
    );
    assert_eq!(heard.paths(), ["/attributed"]);
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

/// Below router scope the 500 is declared on exactly the operations the
/// recovery covers: a group's and an endpoint's, and on neither sibling
/// mounted beside them without it.
#[test]
fn recovery_below_the_router_declares_its_500_on_what_it_covers_alone() {
    let at = |path: &str| PathTemplate::parse(path).expect("a usable path template");

    let document = Router::<()>::new()
        .group(
            Group::<()>::new("/guarded")
                .catch_panics()
                .mount(kynos::routes![boom]),
        )
        .group(Group::<()>::new("/bare").mount(kynos::routes![also_boom]))
        .mount((
            EndpointBuilder::<(), _, _>::new(OpenApiMethod::Get, at("/guarded"), guarded_endpoint)
                .catch_panics(),
            EndpointBuilder::<(), _, _>::new(OpenApiMethod::Get, at("/bare"), bare_endpoint),
        ))
        .openapi()
        .expect("a describable router");
    let document = serde_json::to_value(&document).expect("a serializable document");

    for (path, recovered) in [
        ("/guarded/boom", true),
        ("/bare/boom", false),
        ("/guarded", true),
        ("/bare", false),
    ] {
        let declared = &document["paths"][path]["get"]["responses"]["500"];
        if recovered {
            assert_eq!(
                declared["description"], "the operation failed unexpectedly and was recovered",
                "{path}"
            );
        } else {
            assert!(declared.is_null(), "{path} declares {declared}");
        }
    }
}
