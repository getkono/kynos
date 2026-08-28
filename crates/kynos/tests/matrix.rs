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

use std::{num::NonZeroUsize, time::Duration};

use kynos::{
    Router,
    error::rejection::AuthRejection,
    http::StatusCode,
    middleware::{
        cors::Cors,
        limits::{BodySize, Concurrency, Timeout},
        rate_limit::{Decision, QuotaPolicy, RateLimit, RateLimitPolicy, ServiceLimit},
        request_id::RequestId,
    },
    prelude::*,
    response::{
        headers::WithHeaders,
        status::{NoContent, Redirect},
    },
    router::operation::Route,
    security::{
        Authenticates, Authenticator,
        auth::{Auth, MaybeAuth},
        carrier::{ApiKey, BearerToken},
        schemes::Bearer,
    },
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
    async fn authenticate(&self, presented: BearerToken, _: &C) -> Result<Caller, AuthRejection> {
        // No `strip_prefix("Bearer ")` here, and that is the point: the scheme
        // said where its credential travels, so this only says what the token
        // means.
        match presented.as_str() {
            "tok_ok" => Ok(Caller {
                subject: "user-1".to_owned(),
            }),
            "tok_banned" => Err(AuthRejection::Forbidden),
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

/// A machine key, carried in a field the *attribute* names.
///
/// `Bearer` above is a type Kynos ships, so its carrier is one Kynos wrote. This
/// one's carrier is emitted by the derive from `in` and `name`, which is the
/// half a hand-written finder could get wrong — and the half nothing else here
/// exercises end to end.
#[derive(kynos::SecurityScheme)]
#[security(api_key(in = "header", name = "X-Api-Key"))]
#[security(name = "ServiceKey", credential = Caller)]
struct ServiceKey;

/// Verifies an API key against one issued value.
struct Keys;

impl<C: Sync> Authenticator<ServiceKey, C> for Keys {
    async fn authenticate(&self, presented: ApiKey, _: &C) -> Result<Caller, AuthRejection> {
        // Constant time, because this is a shared secret compared against a
        // stored one — the case `constant_time_eq` exists for.
        let matches = |issued: &[u8]| {
            kynos::security::constant_time_eq(presented.as_str().as_bytes(), issued)
        };

        if matches(b"k_ok") {
            Ok(Caller {
                subject: "integration-1".to_owned(),
            })
        } else if matches(b"k_revoked") {
            // A key that is known and no longer permitted, which is how this
            // operation reaches the 403 `Auth<S>` declares. Without it the
            // status would be a promise this fixture could not keep — and
            // `assert_declared_responses_covered` says so.
            Err(AuthRejection::Forbidden)
        } else {
            Err(AuthRejection::unauthenticated())
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

impl Authenticates<ServiceKey> for App {
    type Authenticator = Keys;

    fn authenticator(&self) -> &Self::Authenticator {
        &Keys
    }
}

/// A rate limit that allows the first request and denies afterwards.
#[derive(Clone, Debug, Default)]
struct AllowsOnce {
    seen: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    policies: Vec<QuotaPolicy>,
}

impl AllowsOnce {
    fn new() -> Self {
        Self {
            seen: std::sync::Arc::default(),
            policies: vec![QuotaPolicy {
                name: "fixture".into(),
                quota: 1,
                window: Some(Duration::from_secs(60)),
                unit: kynos::middleware::rate_limit::QuotaUnit::Requests,
            }],
        }
    }

    fn limit(remaining: u64) -> ServiceLimit {
        ServiceLimit {
            name: "fixture".into(),
            quota: 1,
            remaining,
            reset: Duration::from_secs(60),
        }
    }
}

impl RateLimitPolicy<App> for AllowsOnce {
    fn advertised(&self) -> &[QuotaPolicy] {
        &self.policies
    }

    async fn check(&self, _: &kynos::http::Request, _: Route<'_>, _: &App) -> Decision {
        let seen = self.seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if seen == 0 {
            Decision::allow(Self::limit(0))
        } else {
            Decision::deny(Duration::from_secs(60), Self::limit(0))
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

/// Guarded optionally, which declares a security shape `Auth` cannot.
///
/// `[{}, {Bearer: []}]` — the empty requirement first. Here because the
/// conformance harness reads what the *document* says as well as what the
/// service sends, and this is the one operation whose security list has two
/// members.
#[kynos::get("/feed")]
async fn feed(caller: MaybeAuth<Bearer<Caller>>) -> Json<User> {
    Json(User {
        id: 2,
        name: caller
            .into_inner()
            .map_or_else(|| "anonymous".to_owned(), |caller| caller.subject),
    })
}

/// Guarded by a credential the derive wrote the carrier for.
#[kynos::get("/usage")]
async fn usage(Auth(caller): Auth<ServiceKey>) -> Json<User> {
    Json(User {
        id: 3,
        name: caller.subject,
    })
}

/// The one operation that sets a cookie.
///
/// On a group of its own like every other declaring interceptor, because
/// `SetCookieHeaders` is `DESCRIBED` and lands on the successful responses the
/// operation declares — at router scope it would ride on a handler-produced
/// 4xx that never declared it.
#[cfg(feature = "cookie")]
#[kynos::get("/visit")]
async fn visit() -> NoContent {
    NoContent
}

/// The one operation behind a conditional guard.
///
/// On a group of its own like every other declaring interceptor: `Conditional`
/// contributes a 304, and a status is a promise every covered operation has to
/// keep.
#[cfg(feature = "cache")]
#[kynos::get("/revalidated")]
async fn revalidated() -> WithHeaders<Json<User>, kynos::http::etag::ETag> {
    WithHeaders::new(
        Json(User {
            id: 9,
            name: "stable".to_owned(),
        }),
        kynos::http::etag::ETag::strong("v1"),
    )
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

/// The one operation behind the cross-site guard.
#[kynos::post("/guarded/write")]
async fn under_csrf() -> NoContent {
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
    let router = Router::<App>::new()
        // Router scope: two interceptors that add headers and declare no
        // status, so they cover every operation without adding a response any
        // of them would have to produce.
        .intercept(RequestId::new())
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .mount(kynos::routes![
            get_user,
            list_users,
            create_user,
            moved,
            me,
            feed,
            usage
        ])
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
                .intercept(Concurrency::new(
                    NonZeroUsize::new(1).expect("one is not zero"),
                ))
                .mount(kynos::routes![under_capacity]),
        )
        .group(
            kynos::router::group::Group::<App>::new("/")
                .intercept(RateLimit::new(AllowsOnce::new()))
                .mount(kynos::routes![under_rate_limit]),
        )
        .group(
            kynos::router::group::Group::<App>::new("/")
                .intercept(kynos::middleware::csrf::Csrf::new())
                .mount(kynos::routes![under_csrf]),
        );

    // A ranged file, which is the one layer whose success has three statuses.
    // Mounted here rather than tested only in `tests/assets.rs`, because the
    // 206 and the 416 are a *document* claim about a response nothing else in
    // this workspace checks against a live exchange.
    #[cfg(feature = "assets")]
    let router = router.group(
        kynos::router::group::Group::<App>::new("/files")
            .mount(kynos::router::assets::AssetSet::embedded(DOWNLOAD).no_index()),
    );

    #[cfg(feature = "cache")]
    let router = router.group(
        kynos::router::group::Group::<App>::new("/")
            .intercept(kynos::middleware::conditional::Conditional::new())
            .mount(kynos::routes![revalidated]),
    );

    // Mounted here rather than only in `tests/docs.rs`, because the reference
    // page is the one described `text/html` 200 in the workspace and nothing
    // else checks a described HTML response against a live exchange.
    #[cfg(feature = "docs")]
    let router = router.docs(kynos::router::docs::Docs::scalar());

    #[cfg(feature = "cookie")]
    let router = router.group(
        kynos::router::group::Group::<App>::new("/")
            .intercept(kynos::middleware::cookies::SetCookies::new(vec![
                kynos::response::cookie::Cookie::new("kynos_locale", "en").path("/"),
                kynos::response::cookie::Cookie::new("kynos_seen", "1").path("/"),
            ]))
            .mount(kynos::routes![visit]),
    );

    router.build(App)
}

/// One file, embedded by hand rather than by `assets!`, so this target needs
/// no fixture directory of its own.
#[cfg(feature = "assets")]
const DOWNLOAD: &[kynos::router::assets::Asset] = &[kynos::router::assets::Asset::embedded(
    "report.bin",
    b"0123456789",
    "\"report-v1\"",
)];

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
    #[cfg(feature = "assets")]
    exercise_the_ranges(&client).await;

    #[cfg(feature = "docs")]
    exercise_the_reference(&client).await;

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

    // Both halves of the optional guard: anonymity is a success, and so is a
    // credential. Without the first, `[{}, ...]` would be a declaration nothing
    // produced.
    client
        .get("/feed")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client
        .get("/feed")
        .header("authorization", "Bearer tok_ok")
        .send()
        .await
        .assert_status(StatusCode::OK);

    // The derived carrier, reading the field the attribute named.
    client
        .get("/usage")
        .header("x-api-key", "k_ok")
        .send()
        .await
        .assert_status(StatusCode::OK);

    #[cfg(feature = "cookie")]
    client
        .get("/visit")
        .send()
        .await
        .assert_status(StatusCode::NO_CONTENT);

    // Both statuses the conditional guard declares. Without the second the 304
    // would be a promise this fixture could not keep, which is what
    // `assert_declared_responses_covered` exists to find.
    #[cfg(feature = "cache")]
    {
        client
            .get("/revalidated")
            .send()
            .await
            .assert_status(StatusCode::OK);

        client
            .get("/revalidated")
            .header("if-none-match", "\"v1\"")
            .send()
            .await
            .assert_status(StatusCode::NOT_MODIFIED);
    }
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

    // A credential that is present and wrong is a 401 even where the guard is
    // optional: only *absence* is anonymity.
    client
        .get("/feed")
        .header("authorization", "Bearer tok_unknown")
        .send()
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    client
        .get("/feed")
        .header("authorization", "Bearer tok_banned")
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);

    // The derived carrier reads the declared field, so a key in the wrong one
    // is absent rather than accepted.
    client
        .get("/usage")
        .header("x-api-token", "k_ok")
        .send()
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    client
        .get("/usage")
        .header("x-api-key", "k_wrong")
        .send()
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    client
        .get("/usage")
        .header("x-api-key", "k_revoked")
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
        .assert_status(StatusCode::REQUEST_TIMEOUT);

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

    // The 403 this declares has to be produced, or `assert_declared_responses_covered`
    // reports a response the document predicts and nothing exercises.
    client
        .post("/guarded/write")
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);

    client
        .post("/guarded/write")
        .header("sec-fetch-site", "same-origin")
        .send()
        .await
        .assert_status(StatusCode::NO_CONTENT);

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

    let listing = document.paths.items["/users"]
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

    let listing = document.paths.items["/users/{id}"]
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

/// Both halves of the mounted reference.
///
/// Each declares one status and one media type, so this is the whole of what
/// the description claims about them.
#[cfg(feature = "docs")]
async fn exercise_the_reference(client: &TestClient<App>) {
    client
        .get("/docs")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client
        .get("/openapi.json")
        .send()
        .await
        .assert_status(StatusCode::OK);
}

/// Every status a ranged file can answer with, so the document's 200, 206, 304
/// and 416 are each checked against a response that actually happened.
#[cfg(feature = "assets")]
async fn exercise_the_ranges(client: &TestClient<App>) {
    client
        .get("/files/report.bin")
        .send()
        .await
        .assert_status(StatusCode::OK);

    client
        .get("/files/report.bin")
        .header("range", "bytes=2-5")
        .send()
        .await
        .assert_status(StatusCode::PARTIAL_CONTENT);

    client
        .get("/files/report.bin")
        .header("range", "bytes=99-")
        .send()
        .await
        .assert_status(StatusCode::RANGE_NOT_SATISFIABLE);

    client
        .get("/files/report.bin")
        .header("if-none-match", "\"report-v1\"")
        .send()
        .await
        .assert_status(StatusCode::NOT_MODIFIED);
}
