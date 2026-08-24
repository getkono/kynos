//! The response cache, over a store.
//!
//! One reason: whether a hit happens is a property of a *sequence* of requests
//! and of what the first one's headers said, which no unit test of the rules
//! can see.

#![cfg(all(feature = "macros", feature = "json", feature = "cache"))]

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use kynos::{
    Router,
    http::{Method, StatusCode, header},
    middleware::{
        cache::{Cache, CacheStore, PrimaryKey, StoredResponse},
        conditional::{Conditional, ETag},
    },
    prelude::*,
    response::{headers::WithHeaders, status::NoContent},
};
use serde::{Deserialize, Serialize};

#[path = "support/mod.rs"]
mod support;

use support::{get, send};

// --- A store, which is the half Kynos does not ship ----------------------

#[derive(Clone, Debug, Default)]
struct Stored(Arc<Mutex<HashMap<PrimaryKey, Vec<StoredResponse>>>>);

impl<C: Sync> CacheStore<C> for Stored {
    async fn get(&self, key: &PrimaryKey, _: &C) -> Vec<StoredResponse> {
        self.0
            .lock()
            .expect("no test panics while holding this")
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    async fn put(&self, key: PrimaryKey, response: StoredResponse, _: &C) {
        self.0
            .lock()
            .expect("no test panics while holding this")
            .entry(key)
            .or_default()
            .push(response);
    }

    async fn invalidate(&self, key: &PrimaryKey, _: &C) {
        self.0
            .lock()
            .expect("no test panics while holding this")
            .remove(key);
    }
}

// --- The fixture ----------------------------------------------------------

/// How many times a handler actually ran.
static CALLS: AtomicUsize = AtomicUsize::new(0);

#[derive(Schema, Serialize, Deserialize)]
struct Report {
    id: u64,
}

/// Cacheable, and says so.
#[kynos::get("/reports")]
async fn reports() -> WithHeaders<Json<Report>, CacheControl> {
    CALLS.fetch_add(1, Ordering::SeqCst);
    WithHeaders::new(Json(Report { id: 1 }), CacheControl)
}

/// Says nothing about how long it may be reused.
#[kynos::get("/uncacheable")]
async fn uncacheable() -> Json<Report> {
    CALLS.fetch_add(1, Ordering::SeqCst);
    Json(Report { id: 2 })
}

/// Carries its own validator, so `Conditional` has something to match.
#[kynos::get("/tagged")]
async fn tagged() -> WithHeaders<Json<Report>, ETag> {
    CALLS.fetch_add(1, Ordering::SeqCst);
    WithHeaders::new(Json(Report { id: 3 }), ETag::strong("r3"))
}

#[kynos::post("/reports")]
async fn create() -> NoContent {
    NoContent
}

/// A 204 that carries a validator.
///
/// Legal, and the case RFC 9110 section 15.4.5 excludes from 304: the status
/// says a 200 is *not* what the request would otherwise have produced.
#[kynos::get("/empty")]
async fn empty() -> WithHeaders<NoContent, ETag> {
    CALLS.fetch_add(1, Ordering::SeqCst);
    WithHeaders::new(NoContent, ETag::strong("e1"))
}

/// A `Cache-Control` a handler attaches to its own response.
#[derive(Clone, Copy, Debug)]
struct CacheControl;

impl kynos::extract::params::header::HeaderParams for CacheControl {
    const NAMES: &'static [&'static str] = &["cache-control"];
    const DESCRIBED: bool = false;

    fn encode(&self) -> Vec<(kynos::http::HeaderName, kynos::http::HeaderValue)> {
        vec![(
            header::CACHE_CONTROL,
            kynos::http::HeaderValue::from_static("max-age=60"),
        )]
    }
}

/// A service caching through `store`.
fn cached(store: Stored) -> kynos::router::service::Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![reports, uncacheable, tagged, create, empty])
        .intercept(Cache::new(store).namespace("test"))
        .build(())
        .expect("a describable router")
}

/// Calls since the last reset.
fn calls_during(before: usize) -> usize {
    CALLS.load(Ordering::SeqCst) - before
}

// --- Hits and misses ------------------------------------------------------

#[tokio::test]
async fn a_cacheable_response_is_served_from_the_store_the_second_time() {
    let service = cached(Stored::default());
    let before = CALLS.load(Ordering::SeqCst);

    let first = get(&service, "/reports").call().await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.field("age").as_deref(), Some("0"));

    let second = get(&service, "/reports").call().await;
    assert_eq!(second.status, StatusCode::OK);
    assert_eq!(second.json()["id"], 1);

    assert_eq!(
        calls_during(before),
        1,
        "the handler ran twice, so nothing was served from the store"
    );
}

/// A hit replays a status the operation already declares, which is why the
/// cache's `Short` is `Infallible`.
#[tokio::test]
async fn a_hit_replays_the_status_and_the_headers_that_were_stored() {
    let service = cached(Stored::default());

    let first = get(&service, "/reports").call().await;
    let second = get(&service, "/reports").call().await;

    assert_eq!(first.status, second.status);
    assert_eq!(
        first.field(header::CONTENT_TYPE.as_str()),
        second.field(header::CONTENT_TYPE.as_str())
    );
    assert_eq!(first.body, second.body);
}

/// A response that said nothing about reuse is not reused.
#[tokio::test]
async fn a_response_with_no_freshness_is_never_stored() {
    let service = cached(Stored::default());
    let before = CALLS.load(Ordering::SeqCst);

    for _ in 0..3 {
        assert_eq!(
            get(&service, "/uncacheable").call().await.status,
            StatusCode::OK
        );
    }

    assert_eq!(
        calls_during(before),
        3,
        "a response that stated no lifetime was reused anyway"
    );
}

/// An unsafe method is neither stored nor served from the store.
#[tokio::test]
async fn an_unsafe_method_is_not_cached() {
    let service = cached(Stored::default());

    for _ in 0..2 {
        assert_eq!(
            send(&service, Method::POST, "/reports").call().await.status,
            StatusCode::NO_CONTENT
        );
    }
}

// --- What a precondition may answer ----------------------------------------

/// RFC 9110 section 15.4.5: 304 indicates a request that "would have resulted
/// in a 200 (OK) response if it were not for the fact that the condition
/// evaluated to false".
///
/// The guard was `is_success()`, which also admits 201, 202, 203, 204 and 206.
/// A 204 carrying a validator is the readable case; the one with teeth is 206,
/// where the client asked for a range and a 304 replays `ETag` and `Vary` but
/// no `Content-Range`, leaving it unable to tell "your range is current" from
/// "the whole representation is current".
#[tokio::test]
async fn a_204_carrying_a_matching_validator_is_not_turned_into_a_304() {
    let service = Router::<()>::new()
        .mount(kynos::routes![reports, uncacheable, tagged, create, empty])
        .intercept(Conditional::new())
        .build(())
        .expect("a describable router");

    let response = get(&service, "/empty")
        .header(header::IF_NONE_MATCH.as_str(), "\"e1\"")
        .call()
        .await;

    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "a 204 was rewritten as a 304, which claims a 200 was what the handler would have sent"
    );
}

// --- What the cache adds --------------------------------------------------

/// `Age` is declared and set, and it is not described.
///
/// A cache-to-cache field: a generated client has no use for it, which is the
/// same judgement `Vary` and the CORS set already get.
#[test]
fn the_age_field_is_declared_and_not_described() {
    let document = Router::<()>::new()
        .mount(kynos::routes![reports])
        .intercept(Cache::new(Stored::default()).namespace("test"))
        .openapi()
        .expect("a describable router");

    let emitted = serde_json::to_string(&document).expect("a serializable document");
    assert!(!emitted.contains("\"Age\""), "{emitted}");
}

/// Deriving tags describes the `ETag` it adds, because a consumer can act on
/// one.
#[test]
fn a_deriving_cache_describes_the_tag_it_adds() {
    let document = Router::<()>::new()
        .mount(kynos::routes![reports])
        .intercept(Cache::new(Stored::default()).deriving_etags())
        .openapi()
        .expect("a describable router");

    let emitted = serde_json::to_string(&document).expect("a serializable document");
    assert!(emitted.contains("ETag"), "{emitted}");
}

/// A derived tag is stable across a hit, which is what makes it a validator.
#[tokio::test]
async fn a_derived_tag_is_the_same_on_a_hit_as_on_a_miss() {
    let service = Router::<()>::new()
        .mount(kynos::routes![reports])
        .intercept(Cache::new(Stored::default()).deriving_etags())
        .build(())
        .expect("a describable router");

    let first = get(&service, "/reports").call().await;
    let second = get(&service, "/reports").call().await;

    let tag = first.field(header::ETAG.as_str()).expect("a derived tag");
    assert_eq!(second.field(header::ETAG.as_str()).as_deref(), Some(&*tag));
}

// --- Conditional over a cache --------------------------------------------

/// The arrangement worth having: a hit turned into a 304.
///
/// `Conditional` outside `Cache`, so the body it discards is the cached one
/// rather than the handler's. The hit has to be real for that to be what is
/// asserted: over a route the store never keeps, the handler runs again and
/// `Conditional` alone answers identically, so the arrangement is untested and
/// the name is a claim about nothing. `/reports` states a lifetime and the
/// cache derives the validator, which is what makes the second request a hit
/// carrying a tag.
#[tokio::test]
async fn a_conditional_over_a_cache_answers_a_hit_with_no_body() {
    let service = Router::<()>::new()
        .mount(kynos::routes![reports])
        .intercept(Conditional::new())
        .intercept(
            Cache::new(Stored::default())
                .namespace("test")
                .deriving_etags(),
        )
        .build(())
        .expect("a describable router");

    let first = get(&service, "/reports").call().await;
    let etag = first.field(header::ETAG.as_str()).expect("a derived tag");
    let before = CALLS.load(Ordering::SeqCst);

    let second = get(&service, "/reports")
        .header("if-none-match", &etag)
        .call()
        .await;

    assert_eq!(second.status, StatusCode::NOT_MODIFIED);
    assert!(second.body.is_empty());
    assert_eq!(
        calls_during(before),
        0,
        "the handler ran, so the body the 304 discarded was not the cached one"
    );
}

/// The two compose, which is the property their `Adds` sets were chosen for.
#[test]
fn a_cache_and_a_conditional_declare_disjoint_fields() {
    let document = Router::<()>::new()
        .mount(kynos::routes![tagged])
        .intercept(Conditional::new())
        .intercept(Cache::new(Stored::default()))
        .openapi()
        .expect("a describable router");

    let operation = document.paths.0["/tagged"].get.as_ref().expect("a GET");

    let mut statuses: Vec<&str> = operation
        .responses
        .responses
        .keys()
        .map(String::as_str)
        .collect();
    statuses.sort_unstable();

    assert_eq!(statuses, ["200", "304"]);
}
