//! The whole owned-layer matrix, checked against the description it emits.
//!
//! This is the only assertion in the workspace that fails when the **document**
//! is wrong rather than when the code is. Everything else tests the framework's
//! types; this tests the framework's claim.
//!
//! Two assertions, opposite in direction and both needed:
//!
//! * `assert_conformance` — nothing happened that the document did not predict.
//!   Every observed response is checked against the `Responses` entry for its
//!   operation and status: that the status is declared, that the body validates
//!   against the declared schema, and that every declared required header was
//!   sent.
//! * `assert_declared_responses_covered` — nothing the document predicts has
//!   gone unexercised. This is coverage over the *contract*, and it is what
//!   makes the fixture below the shape it is: every interceptor sits on a group
//!   holding exactly one operation, because a limit mounted at the router would
//!   declare its status on all of them and every one would then have to be
//!   made to produce it.
//!
//! Its narrow sibling has already earned its keep once —
//! [`error/rejection.rs`](../src/error/rejection.rs) records that
//! `assert_conformance` is what caught `BodyRejection` declaring a 413 no
//! operation could produce, a class of defect no line-coverage number shows.

#![cfg(all(
    feature = "macros",
    feature = "json",
    feature = "test-util",
    feature = "openapi31"
))]

use std::time::Duration;

use kynos::{
    Router,
    error::rejection::AuthRejection,
    http::{Parts, StatusCode, header},
    middleware::{
        cors::Cors,
        limits::{BodySize, Concurrency, Timeout},
        rate_limit::{Decision, RateLimit, RateLimitPolicy},
        request_id::RequestId,
    },
    prelude::*,
    response::{
        headers::WithHeaders,
        status::{NoContent, Redirect},
    },
    security::{Authenticates, Authenticator, auth::Auth, schemes::Bearer},
    test::TestClient,
};
use serde::{Deserialize, Serialize};

// --- The types the operations exchange -----------------------------------

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

/// What `/users/{id}` captures.
#[derive(Schema, PathParams)]
struct UserPath {
    #[allow(dead_code)]
    id: u64,
}

/// What `/users` reads from the request target.
#[derive(Schema, QueryParams)]
struct UserQuery {
    limit: Option<u32>,
}

/// A declared response header group, so a `WithHeaders` return has something
/// the description promises and the conformance check can look for.
#[derive(HeaderParams)]
struct Paging {
    #[header(rename = "X-Total-Count")]
    total: u64,
}

/// What creating a user can fail with.
#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum StoreError {
    #[error("that name is already taken")]
    #[problem(status = 409, type = "https://errors.example.com/name-taken")]
    NameTaken,
}

// --- Authentication -------------------------------------------------------

/// What a verified token yields.
#[derive(Clone, Debug)]
struct Caller {
    subject: String,
}

/// Verifies against a fixed table.
///
/// `Forbidden` for a known-but-banned caller, which is how one operation
/// reaches both statuses `Auth<S>` declares. Without it the 403 would be a
/// promise this fixture could not keep, which is the exact defect the second
/// assertion exists to find.
struct Tokens;

impl<C: Sync> Authenticator<Bearer<Caller>, C> for Tokens {
    async fn authenticate(&self, parts: &Parts, _: &C) -> Result<Caller, AuthRejection> {
        match parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
        {
            Some("tok_ok") => Ok(Caller {
                subject: "user-1".to_owned(),
            }),
            Some("tok_banned") => Err(AuthRejection::Forbidden),
            _ => Err(AuthRejection::unauthenticated()),
        }
    }

    async fn authorize(
        &self,
        _: &Caller,
        _: &'static [&'static str],
        _: &C,
    ) -> Result<(), AuthRejection> {
        Ok(())
    }
}

/// The application context.
struct App;

impl Authenticates<Bearer<Caller>> for App {
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Self::Authenticator {
        &Tokens
    }
}

/// A rate limit that allows the first request and denies afterwards.
#[derive(Clone, Debug)]
struct AllowsOnce(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl RateLimitPolicy<App> for AllowsOnce {
    async fn check(&self, _: &kynos::http::Request, _: &App) -> Decision {
        let seen = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if seen == 0 {
            Decision::Allow {
                remaining: 0,
                reset: Duration::from_secs(60),
            }
        } else {
            Decision::Deny {
                retry_after: Duration::from_secs(60),
            }
        }
    }
}

// --- The operations -------------------------------------------------------

/// Fetches one user. A path group, and the 400 a bad capture produces.
#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>) -> Json<User> {
    Json(User {
        id: path.id,
        name: "Ada Lovelace".to_owned(),
    })
}

/// Lists users. A query group, and a `WithHeaders` return.
#[kynos::get("/users")]
async fn list_users(Query(query): Query<UserQuery>) -> WithHeaders<Json<Vec<User>>, Paging> {
    let total = query.limit.unwrap_or(1);

    WithHeaders::new(
        Json(
            (0..total)
                .map(|id| User {
                    id: u64::from(id),
                    name: "Grace Hopper".to_owned(),
                })
                .collect(),
        ),
        Paging {
            total: u64::from(total),
        },
    )
}

/// Creates a user. A body codec, a `Created` wrapper and a declared failure.
#[kynos::post("/users")]
async fn create_user(Json(user): Json<User>) -> Result<Created<Json<User>>, StoreError> {
    if user.name == "taken" {
        return Err(StoreError::NameTaken);
    }

    Ok(Created::at(
        get_user::relative_uri(UserPath { id: user.id }),
        Json(user),
    ))
}

/// A permanent redirect, which is a status no handler body produces.
#[kynos::get("/old-users")]
async fn moved() -> Redirect<308> {
    Redirect::to("/users")
}

/// Guarded by a credential, and reaching both statuses the guard declares.
#[kynos::get("/me")]
async fn me(Auth(caller): Auth<Bearer<Caller>>) -> Json<User> {
    Json(User {
        id: 1,
        name: caller.subject,
    })
}

/// The one operation under a body limit.
#[kynos::post("/limits/size")]
async fn under_size_limit(Json(user): Json<User>) -> NoContent {
    let _ = user;
    NoContent
}

/// The one operation under a timeout. Sleeps when asked to.
#[kynos::get("/limits/time")]
async fn under_timeout(Query(query): Query<UserQuery>) -> NoContent {
    if query.limit == Some(1) {
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    NoContent
}

/// The one operation under a concurrency cap. Holds its slot when asked to.
#[kynos::get("/limits/slots")]
async fn under_capacity(Query(query): Query<UserQuery>) -> NoContent {
    if query.limit == Some(1) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    NoContent
}

/// The one operation under a rate limit.
#[kynos::get("/limits/rate")]
async fn under_rate_limit() -> NoContent {
    NoContent
}

/// The whole matrix: every owned layer, mounted where its declaration lands on
/// exactly the operations that can produce it.
fn service() -> kynos::Result<kynos::router::service::Service<App>> {
    Router::<App>::new()
        // Router scope: two interceptors that add headers and declare no
        // status, so they cover every operation without adding a response any
        // of them would have to produce.
        .intercept(RequestId::new())
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .mount(kynos::routes![get_user, list_users, create_user, moved, me])
        // Group scope: one operation each, because a declared status is a
        // promise every covered operation has to keep.
        .group(
            kynos::router::group::Group::<App>::new("/")
                .intercept(BodySize::new(64))
                .mount(kynos::routes![under_size_limit]),
        )
        .group(
            kynos::router::group::Group::<App>::new("/")
                .intercept(Timeout::new(Duration::from_millis(30)))
                .mount(kynos::routes![under_timeout]),
        )
        .group(
            kynos::router::group::Group::<App>::new("/")
                .intercept(Concurrency::new(1))
                .mount(kynos::routes![under_capacity]),
        )
        .group(
            kynos::router::group::Group::<App>::new("/")
                .intercept(RateLimit::new(1, AllowsOnce(std::sync::Arc::default())))
                .mount(kynos::routes![under_rate_limit]),
        )
        .build(App)
}

// --- The two assertions ---------------------------------------------------

/// Every response this fixture produced is one the description declares, and
/// every response the description declares was produced.
///
/// One test rather than two, because the second assertion reads the *same*
/// recorded exchanges: splitting them would mean driving the whole matrix
/// twice, and a second client would record a second, different set.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_owned_layer_matrix_matches_the_description_it_emits() {
    let client = TestClient::new(service().expect("a describable router"));

    exercise_the_operations(&client).await;
    exercise_the_rejections(&client).await;
    exercise_the_limits(&client).await;

    client.assert_conformance();
    client.assert_declared_responses_covered();
}

/// Every operation's success.
async fn exercise_the_operations(client: &TestClient<App>) {
    client
        .get("/users/42")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client
        .get("/users?limit=2")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client
        .post("/users")
        .json(&User {
            id: 1,
            name: "fresh".to_owned(),
        })
        .send()
        .await
        .assert_status(StatusCode::CREATED);

    client
        .get("/old-users")
        .send()
        .await
        .assert_status(StatusCode::PERMANENT_REDIRECT);

    client
        .get("/me")
        .header("authorization", "Bearer tok_ok")
        .send()
        .await
        .assert_status(StatusCode::OK);
}

/// Every declared way an operation says no.
async fn exercise_the_rejections(client: &TestClient<App>) {
    // A path variable that is not a `u64`.
    client
        .get("/users/not-a-number")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // A query member of the wrong type.
    client
        .get("/users?limit=lots")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // A body that is not JSON at all.
    client
        .post("/users")
        .header("content-type", "application/json")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // A media type the operation never claimed.
    client
        .post("/users")
        .header("content-type", "text/plain")
        .send()
        .await
        .assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Valid JSON of the wrong shape.
    client
        .post("/users")
        .json(&serde_json::json!({ "id": "one", "name": 1 }))
        .send()
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    // The declared application failure.
    client
        .post("/users")
        .json(&User {
            id: 2,
            name: "taken".to_owned(),
        })
        .send()
        .await
        .assert_status(StatusCode::CONFLICT)
        .assert_problem_type("https://errors.example.com/name-taken");

    // Both statuses the credential guard declares.
    client
        .get("/me")
        .send()
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    client
        .get("/me")
        .header("authorization", "Bearer tok_banned")
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);
}

/// Every status an interceptor contributes, on the one operation it covers.
async fn exercise_the_limits(client: &TestClient<App>) {
    client
        .post("/limits/size")
        .json(&User {
            id: 1,
            name: "s".to_owned(),
        })
        .send()
        .await
        .assert_status(StatusCode::NO_CONTENT);

    client
        .post("/limits/size")
        .json(&User {
            id: 1,
            name: "a name comfortably past sixty-four bytes, which this one certainly is"
                .to_owned(),
        })
        .send()
        .await
        .assert_status(StatusCode::PAYLOAD_TOO_LARGE);

    client
        .get("/limits/time")
        .send()
        .await
        .assert_status(StatusCode::NO_CONTENT);

    client
        .get("/limits/time?limit=1")
        .send()
        .await
        .assert_status(StatusCode::GATEWAY_TIMEOUT);

    client
        .get("/limits/slots")
        .send()
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Two overlapping requests, driven with `join!` rather than two spawned
    // tasks: the nextest profile fails a test that leaks one.
    let (held, refused) = tokio::join!(client.get("/limits/slots?limit=1").send(), async {
        tokio::time::sleep(Duration::from_millis(40)).await;
        client.get("/limits/slots").send().await
    });
    held.assert_status(StatusCode::NO_CONTENT);
    refused.assert_status(StatusCode::SERVICE_UNAVAILABLE);

    client
        .get("/limits/rate")
        .send()
        .await
        .assert_status(StatusCode::NO_CONTENT);

    client
        .get("/limits/rate")
        .send()
        .await
        .assert_status(StatusCode::TOO_MANY_REQUESTS);

    // Each limited operation still carries the rejections its own extractors
    // declare, and a status declared on one operation is not exercised by
    // producing it on another.
    client
        .post("/limits/size")
        .header("content-type", "application/json")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    client
        .post("/limits/size")
        .header("content-type", "text/plain")
        .send()
        .await
        .assert_status(StatusCode::UNSUPPORTED_MEDIA_TYPE);

    client
        .post("/limits/size")
        .json(&serde_json::json!({ "id": "one", "name": 1 }))
        .send()
        .await
        .assert_status(StatusCode::UNPROCESSABLE_ENTITY);

    client
        .get("/limits/time?limit=lots")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    client
        .get("/limits/slots?limit=lots")
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);
}

/// A response header a group declares is one the conformance check requires,
/// so this pins that the group above is actually described rather than merely
/// sent.
#[test]
fn a_declared_response_header_reaches_the_description() {
    let service = service().expect("a describable router");
    let document = service.openapi();

    let listing = document.paths.0["/users"]
        .get
        .as_ref()
        .expect("a GET operation");

    assert!(
        listing.responses.responses["200"]
            .as_item()
            .expect("an inline response")
            .headers
            .contains_key("X-Total-Count"),
        "a header group with `DESCRIBED = true` was sent and not declared"
    );
}

/// An interceptor's response header is declared where a consumer will look for
/// it.
///
/// `ErasedInterceptor::describe` files the header under `StatusPattern::Success`
/// — the `2XX` key. A consumer resolving an observed 200 takes the *exact* key
/// first, per the precedence the specification gives, so it reaches the `200`
/// entry and never sees the header. The operation therefore sends a header its
/// description declares nowhere a reader will find it, and the `2XX` entry is a
/// response no service can ever produce.
///
/// This is the failure `assert_declared_responses_covered` reports above: nine
/// `2XX` keys, one per operation, none reachable.
#[test]
fn an_interceptors_response_header_is_declared_where_a_consumer_resolves_it() {
    let service = service().expect("a describable router");
    let document = service.openapi();

    let listing = document.paths.0["/users/{id}"]
        .get
        .as_ref()
        .expect("a GET operation");

    let success = listing.responses.responses["200"]
        .as_item()
        .expect("an inline 200");

    assert!(
        success.headers.contains_key("X-Request-Id"),
        "the 200 a consumer resolves declares {:?}, and the header an \
         interceptor sets is filed under a key nothing resolves to",
        success.headers.keys().collect::<Vec<_>>()
    );
}
