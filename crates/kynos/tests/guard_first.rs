//! Where an operation's guard runs once a scope asks for it first.
//!
//! One reason: `guard_first` moves the guard to a point in a chain the router
//! assembles from every scope covering the operation, so only a built service
//! shows which interceptors a refusal passed through, how often the credential
//! was checked, and that the description did not move with it.

#![cfg(all(feature = "macros", feature = "json"))]

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use kynos::{
    Router,
    error::rejection::AuthRejection,
    extract::body::text::Text,
    http::{Request, StatusCode, header},
    middleware::{Continued, Interceptor, Next, limits::body_size::BodySize},
    response::status::NoContent,
    router::group::Group,
    security::{
        Authenticates, Authenticator, Unscoped,
        auth::{Auth, MaybeAuth},
        carrier::ApiKey,
    },
};

#[path = "support/mod.rs"]
mod support;

use support::{get, post};

// --- The fixture -----------------------------------------------------------

/// What the guard and the interceptors saw, shared with the built service.
#[derive(Clone, Default)]
struct Gate {
    /// How many times the authenticator was asked.
    checks: Arc<AtomicUsize>,
    /// Each [`Witness`] a request passed through, outermost first.
    passed: Arc<Mutex<Vec<&'static str>>>,
}

impl Gate {
    fn checks(&self) -> usize {
        self.checks.load(Ordering::SeqCst)
    }

    fn passed(&self) -> Vec<&'static str> {
        self.passed.lock().expect("an unpoisoned log").clone()
    }
}

/// Who presented the key.
#[derive(Clone, Debug)]
struct Holder;

/// A key in a field of its own, answered with the `ApiKey` challenge.
#[derive(kynos::SecurityScheme)]
#[security(api_key(in = "header", name = "X-Api-Key"))]
#[security(name = "UploadKey", credential = Holder)]
struct UploadKey;

/// Accepts `k_ok`, and panics on `k_panic`, counting every check.
struct Keys;

impl Authenticator<UploadKey, Gate> for Keys {
    async fn authenticate(&self, presented: ApiKey, gate: &Gate) -> Result<Holder, AuthRejection> {
        gate.checks.fetch_add(1, Ordering::SeqCst);
        match presented.as_str() {
            "k_ok" => Ok(Holder),
            "k_panic" => panic!("the credential store failed"),
            _ => Err(AuthRejection::unauthenticated()),
        }
    }

    async fn authorize(&self, _: &Holder, _: &[Unscoped], _: &Gate) -> Result<(), AuthRejection> {
        Ok(())
    }
}

impl Authenticates<UploadKey> for Gate {
    type Authenticator = Keys;

    fn authenticator(&self) -> &Keys {
        &Keys
    }
}

/// Records that a request passed through it, under its name.
struct Witness(&'static str);

impl Interceptor<Gate> for Witness {
    type Reads = ();
    type Adds = ();
    type Short = std::convert::Infallible;

    async fn intercept(
        &self,
        request: Request,
        (): (),
        gate: &Gate,
        next: Next<'_, Gate>,
    ) -> Result<Continued, std::convert::Infallible> {
        gate.passed.lock().expect("an unpoisoned log").push(self.0);
        Ok(next.run(request).await)
    }
}

/// Takes a body behind the key.
#[kynos::post("/uploads")]
async fn upload(Auth(_): Auth<UploadKey>, Text(_): Text) -> NoContent {
    NoContent
}

/// Honours the key without demanding it, and says which it got.
#[kynos::get("/feed")]
async fn feed(MaybeAuth(holder): MaybeAuth<UploadKey>) -> Text {
    Text(
        if holder.is_some() {
            "holder"
        } else {
            "anonymous"
        }
        .to_owned(),
    )
}

/// Takes no guard at all.
#[kynos::get("/open")]
async fn open() -> NoContent {
    NoContent
}

/// A group of the three, built over `gate`. A macro, since each group's
/// interceptors are a type of their own.
macro_rules! served {
    ($group:expr, $gate:expr) => {
        Router::new()
            .group($group.mount(kynos::routes![upload, feed, open]))
            .build($gate.clone())
            .expect("a describable router")
    };
}

// --- Where the guard runs --------------------------------------------------

/// The issue's case: an anonymous upload is refused by the guard's own 401,
/// challenge included, before the group's body limit reads anything — where
/// without `guard_first` the limit answers first.
#[tokio::test]
async fn an_anonymous_upload_is_refused_before_the_body_limit_reads_it() {
    let oversized = "x".repeat(64);
    let limited = |group: Group<Gate>| group.intercept(BodySize::new(16));

    let gate = Gate::default();
    let first = served!(limited(Group::new("/api").guard_first()), gate);
    let refused = post(&first, "/api/uploads")
        .header("content-type", "text/plain")
        .body(oversized.clone())
        .call()
        .await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        refused.field(header::WWW_AUTHENTICATE.as_str()).as_deref(),
        Some("ApiKey")
    );

    let gate = Gate::default();
    let last = served!(limited(Group::new("/api")), gate);
    let limited = post(&last, "/api/uploads")
        .header("content-type", "text/plain")
        .body(oversized)
        .call()
        .await;
    assert_eq!(
        limited.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "the default order changed"
    );
}

/// The point is where the call sits: an interceptor applied before
/// `guard_first` wraps the refusal, and one applied after never sees it.
#[tokio::test]
async fn the_guard_runs_between_the_interceptors_either_side_of_the_call() {
    let gate = Gate::default();
    let group = Group::new("/api")
        .intercept(Witness("outer"))
        .guard_first()
        .intercept(Witness("inner"));
    let service = served!(group, gate);

    let refused = post(&service, "/api/uploads").call().await;

    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(gate.passed(), ["outer"]);
    assert_eq!(gate.checks(), 0, "an absent key is refused unasked");
}

/// An admitted request is checked once, ahead of the interceptors, and the
/// handler's guard argument receives that credential rather than asking again.
#[tokio::test]
async fn an_admitted_request_is_checked_once() {
    let gate = Gate::default();
    let service = served!(
        Group::new("/api").guard_first().intercept(Witness("inner")),
        gate
    );

    let admitted = post(&service, "/api/uploads")
        .header("x-api-key", "k_ok")
        .header("content-type", "text/plain")
        .body("payload")
        .call()
        .await;

    assert_eq!(admitted.status, StatusCode::NO_CONTENT);
    assert_eq!(gate.checks(), 1);
    assert_eq!(gate.passed(), ["inner"]);
}

/// An optional guard run first hands the handler what it found, absence
/// included, and still refuses a key that is present and wrong.
#[tokio::test]
async fn an_optional_guard_run_first_hands_on_what_it_found() {
    let gate = Gate::default();
    let service = served!(Group::new("/api").guard_first(), gate);

    let anonymous = get(&service, "/api/feed").call().await;
    assert_eq!(anonymous.status, StatusCode::OK);
    assert_eq!(anonymous.text(), "anonymous");

    let holder = get(&service, "/api/feed")
        .header("x-api-key", "k_ok")
        .call()
        .await;
    assert_eq!(holder.text(), "holder");

    let wrong = get(&service, "/api/feed")
        .header("x-api-key", "k_wrong")
        .call()
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(gate.checks(), 2);
}

/// An operation taking no guard is untouched: every interceptor still runs.
#[tokio::test]
async fn an_operation_without_a_guard_keeps_its_chain() {
    let gate = Gate::default();
    let service = served!(
        Group::new("/api").guard_first().intercept(Witness("inner")),
        gate
    );

    let open = get(&service, "/api/open").call().await;

    assert_eq!(open.status, StatusCode::NO_CONTENT);
    assert_eq!(gate.passed(), ["inner"]);
}

// --- Scopes, nested --------------------------------------------------------

/// `Router::guard_first` covers a group's operations, and outranks the
/// group's own point: the guard runs once, at the outermost one asked for.
#[tokio::test]
async fn the_outermost_point_asked_for_wins() {
    let gate = Gate::default();
    let group = Group::new("/api")
        .intercept(Witness("group"))
        .guard_first()
        .mount(kynos::routes![upload]);
    let service = Router::new()
        .intercept(Witness("router"))
        .guard_first()
        .intercept(Witness("router, inside"))
        .group(group)
        .build(gate.clone())
        .expect("a describable router");

    let refused = post(&service, "/api/uploads")
        .header("x-api-key", "k_wrong")
        .call()
        .await;

    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(gate.passed(), ["router"]);
    assert_eq!(gate.checks(), 1);
}

/// A group's point survives being nested: the enclosing router's interceptors
/// still wrap it, and the group's own after the call still do not.
#[tokio::test]
async fn a_groups_point_survives_nesting() {
    let gate = Gate::default();
    let group = Group::new("/api")
        .intercept(Witness("group"))
        .guard_first()
        .intercept(Witness("group, inside"))
        .mount(kynos::routes![upload]);
    let inner = Router::new().intercept(Witness("nested")).group(group);
    let service = Router::new()
        .intercept(Witness("root"))
        .nest("/v1", inner)
        .build(gate.clone())
        .expect("a describable router");

    let refused = post(&service, "/v1/api/uploads").call().await;

    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(gate.passed(), ["root", "nested", "group"]);
}

// --- What does not move ----------------------------------------------------

/// Only when the guard runs moves: the description is byte for byte the one
/// the same group describes without `guard_first`.
#[test]
fn the_description_is_unchanged() {
    let describe = |group: Group<Gate>| {
        Router::new()
            .group(
                group
                    .intercept(BodySize::new(16))
                    .mount(kynos::routes![upload, feed, open]),
            )
            .openapi()
            .expect("a describable router")
            .to_json()
            .expect("a serializable document")
    };

    assert_eq!(
        describe(Group::new("/api").guard_first()),
        describe(Group::new("/api"))
    );
}

/// An endpoint's own recovery covers its guard wherever the guard runs: a
/// panicking authenticator is the operation's 500, not an unwind.
#[test]
fn an_endpoints_recovery_covers_its_guard_run_first() {
    let endpoint = kynos::router::endpoint::builder::EndpointBuilder::new(
        kynos::openapi::Method::Get,
        kynos::openapi::PathTemplate::parse("/feed").expect("valid path"),
        feed,
    )
    .catch_panics();
    let gate = Gate::default();
    let service = Router::new()
        .group(Group::new("/api").guard_first().mount(endpoint))
        .build(gate)
        .expect("a describable router");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(
            get(&service, "/api/feed")
                .header("x-api-key", "k_panic")
                .call(),
        )
    }));
    std::panic::set_hook(hook);

    let reply = caught.expect("the endpoint's recovery caught the guard's panic");
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
}
