//! The interceptor group's tables: [`router`](super::router) with one
//! interceptor mounted at router scope, and the in-memory stores the two
//! that need one are given.

use std::collections::HashMap;
use std::convert::Infallible;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kynos::{
    http::{Request, body::Body, header},
    middleware::{
        cache::{
            Cache,
            store::{CacheStore, PrimaryKey, StoredResponse},
        },
        compression::Compression,
        cookies::SetCookies,
        cors::Cors,
        csrf::Csrf,
        decompression::Decompression,
        limits::{
            body_size::BodySize, body_timeout::BodyTimeout, concurrency::Concurrency,
            timeout::Timeout,
        },
        rate_limit::{
            RateLimit,
            key::ByRoute,
            quota::{Quota, Quotas},
            store::RateLimitStore,
        },
        request_id::{Random, RequestId},
        security_headers::SecurityHeaders,
    },
    response::cookie::Cookie,
};
use tokio::io::AsyncReadExt as _;

use super::{App, built, echo_body, octets, post, router};
use crate::driver::{Driver, Served};

/// The origin the CORS rows are sent from and admit.
pub(crate) const ORIGIN: &str = "https://app.example.com";

/// The table behind CORS admitting [`ORIGIN`].
pub(crate) fn cors() -> Box<dyn Served> {
    built(router().intercept(Cors::new().allow_origins([ORIGIN])))
}

/// The table behind `Csrf`'s defaults.
pub(crate) fn csrf() -> Box<dyn Served> {
    built(router().intercept(Csrf::new()))
}

/// The table behind `RequestId`'s default source, a counter.
pub(crate) fn request_id_counter() -> Box<dyn Served> {
    built(router().intercept(RequestId::new()))
}

/// The table behind `RequestId` drawing identifiers from `Random`.
pub(crate) fn request_id_random() -> Box<dyn Served> {
    built(router().intercept(RequestId::new().source(Random::new())))
}

/// The table behind `SecurityHeaders`' defaults.
pub(crate) fn security_headers() -> Box<dyn Served> {
    built(router().intercept(SecurityHeaders::new()))
}

/// The table behind a one-mebibyte body bound.
pub(crate) fn body_size() -> Box<dyn Served> {
    built(router().intercept(BodySize::new(1_048_576)))
}

/// The table behind a concurrency limit no profiled request reaches.
pub(crate) fn concurrency() -> Box<dyn Served> {
    built(router().intercept(Concurrency::new(
        NonZeroUsize::new(256).expect("a nonzero limit"),
    )))
}

/// The table behind a deadline no profiled request reaches.
pub(crate) fn timeout() -> Box<dyn Served> {
    built(router().intercept(Timeout::new(Duration::from_secs(30))))
}

/// The table behind an idle bound on the request body no profiled request
/// reaches.
pub(crate) fn body_timeout() -> Box<dyn Served> {
    built(router().intercept(BodyTimeout::idle(Duration::from_secs(15))))
}

/// The table decoding request bodies of up to one mebibyte.
pub(crate) fn decompression() -> Box<dyn Served> {
    built(router().intercept(Decompression::new(1_048_576)))
}

/// `echo-post`'s request, its body gzip-coded and labelled so.
pub(crate) fn gzipped_echo() -> Request {
    let plain = octets(echo_body());
    let coded = Driver::Poll.runner().run(async {
        let mut coded = Vec::new();
        async_compression::tokio::bufread::GzipEncoder::new(&plain[..])
            .read_to_end(&mut coded)
            .await
            .expect("an in-memory encode does not fail");
        coded
    });
    let mut request = post("/echo", Body::from_bytes(bytes::Bytes::from(coded)));
    request.headers_mut().insert(
        header::CONTENT_ENCODING,
        header::HeaderValue::from_static("gzip"),
    );
    request
}

/// Counters held in process, as an application without a shared store would
/// hold them.
#[derive(Default)]
struct Counters(Mutex<HashMap<String, u64>>);

impl RateLimitStore for Counters {
    type Error = Infallible;

    async fn read(&self, key: &str) -> Result<u64, Infallible> {
        let counters = self.0.lock().expect("an unpoisoned store");
        Ok(counters.get(key).copied().unwrap_or(0))
    }

    async fn increment(&self, key: &str, by: u64, ttl: Duration) -> Result<u64, Infallible> {
        let _ = ttl;
        let mut counters = self.0.lock().expect("an unpoisoned store");
        let counted = if let Some(counted) = counters.get_mut(key) {
            *counted += by;
            *counted
        } else {
            counters.insert(key.to_owned(), by);
            by
        };
        Ok(counted)
    }
}

/// The table behind a per-route quota no profiled request exhausts.
pub(crate) fn rate_limit() -> Box<dyn Served> {
    built(router().intercept(RateLimit::new(
        Quotas::new(ByRoute, Counters::default()).quota(Quota::per_hour("profile", 1_000_000)),
    )))
}

/// Responses held in process; or, where `retain` is unset, offered every
/// response and keeping none, so that every lookup misses and every miss is
/// offered for storing as a first one would be.
struct Responses {
    retain: bool,
    held: Arc<Mutex<HashMap<PrimaryKey, Vec<StoredResponse>>>>,
}

impl CacheStore<App> for Responses {
    async fn get(&self, key: &PrimaryKey, context: &App) -> Vec<StoredResponse> {
        let _ = context;
        let held = self.held.lock().expect("an unpoisoned store");
        held.get(key).cloned().unwrap_or_default()
    }

    async fn put(&self, key: PrimaryKey, response: StoredResponse, context: &App) {
        let _ = context;
        if self.retain {
            let mut held = self.held.lock().expect("an unpoisoned store");
            let variants = held.entry(key).or_default();
            variants.retain(|stored| !stored.same_variant(&response));
            variants.push(response);
        }
    }

    async fn invalidate(&self, key: &PrimaryKey, context: &App) {
        let _ = context;
        self.held.lock().expect("an unpoisoned store").remove(key);
    }
}

/// The table behind a cache over `store`.
///
/// `json-small` states no freshness, so the cache is given a default one;
/// without it nothing would be stored and both rows would measure the
/// refusal.
fn cached(store: Responses) -> Box<dyn Served> {
    built(router().intercept(Cache::new(store).default_freshness(Duration::from_secs(3_600))))
}

/// The table behind a cache that never holds what it is offered.
pub(crate) fn cache_miss() -> Box<dyn Served> {
    cached(Responses {
        retain: false,
        held: Arc::default(),
    })
}

/// The table behind a cache already holding `/json/small`: one request is
/// served when the service is built, so that whichever request comes next,
/// in a benchmark or a test, is answered from the store.
///
/// # Panics
///
/// When that request was not stored, so that the row would count a miss.
pub(crate) fn cache_hit() -> Box<dyn Served> {
    let held = Arc::default();
    let service = cached(Responses {
        retain: true,
        held: Arc::clone(&held),
    });
    drop(service.serve(
        &Driver::Poll.runner(),
        super::get("/json/small"),
        crate::driver::frames(),
    ));
    assert!(
        !held.lock().expect("an unpoisoned store").is_empty(),
        "the priming request was stored"
    );
    service
}

/// The table setting one session cookie on every response.
pub(crate) fn set_cookies() -> Box<dyn Served> {
    built(router().intercept(SetCookies::new(vec![
        Cookie::new("session", "opaque").http_only().secure(),
    ])))
}

/// The table behind `Compression`'s defaults, its `min_size` included.
pub(crate) fn compression() -> Box<dyn Served> {
    built(router().intercept(Compression::new()))
}

/// The table wrapped in a `tower` layer that forwards and does nothing else.
pub(crate) fn unchecked_layer() -> Box<dyn Served> {
    built(router().layer_unchecked(tower_layer::Identity::new()))
}
