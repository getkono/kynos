//! The shipped rate limiter, driven over a store.
//!
//! One reason: `Quotas` is the half of rate limiting Kynos does own, and its
//! behaviour is a property of a *sequence* of requests rather than of any one.
//! The estimator and the recovery delay are unit-tested where they live; this
//! drives the whole thing over a built service and reads what came back.

#![cfg(all(feature = "macros", feature = "json"))]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use kynos::{
    Router,
    http::StatusCode,
    http::forwarded::TrustedProxies,
    middleware::rate_limit::{
        Quota, Quotas, RateLimit, RateLimitStore, StoreFailure,
        key::{And, ByClientAddress, ByHeader, ByPeerAddress, ByRoute, Shared},
    },
    response::status::NoContent,
};

#[path = "support/mod.rs"]
mod support;

use support::get;

// --- A store, which is the half Kynos does not ship ----------------------

/// A counter store over a map, which is all the trait asks for.
///
/// Fifteen lines, and that is the point: the seam is two methods because
/// anything richer would be unimplementable against Redis. `examples/rate_limit.rs`
/// is the same shape over `moka`, with eviction.
#[derive(Clone, Debug, Default)]
struct Counters(Arc<Mutex<HashMap<String, u64>>>);

/// A store that always fails, for the two failure policies.
#[derive(Clone, Copy, Debug, Default)]
struct Broken;

#[derive(Debug, thiserror::Error)]
#[error("the counter store is unavailable")]
struct Unavailable;

impl RateLimitStore for Counters {
    type Error = Unavailable;

    async fn read(&self, key: &str) -> Result<u64, Self::Error> {
        Ok(self
            .0
            .lock()
            .expect("no test panics while holding this")
            .get(key)
            .copied()
            .unwrap_or(0))
    }

    async fn increment(&self, key: &str, by: u64, _: Duration) -> Result<u64, Self::Error> {
        let mut counters = self.0.lock().expect("no test panics while holding this");
        let counter = counters.entry(key.to_owned()).or_insert(0);
        *counter += by;
        Ok(*counter)
    }
}

impl RateLimitStore for Broken {
    type Error = Unavailable;

    async fn read(&self, _: &str) -> Result<u64, Self::Error> {
        Err(Unavailable)
    }

    async fn increment(&self, _: &str, _: u64, _: Duration) -> Result<u64, Self::Error> {
        Err(Unavailable)
    }
}

// --- The fixture ----------------------------------------------------------

#[kynos::get("/counted")]
async fn counted() -> NoContent {
    NoContent
}

#[kynos::get("/other")]
async fn other() -> NoContent {
    NoContent
}

/// A service limited by `quotas`.
fn limited<S: RateLimitStore>(quotas: Quotas<Shared, S>) -> kynos::router::service::Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![counted, other])
        .intercept(RateLimit::new(quotas))
        .build(())
        .expect("a describable router")
}

/// One shared bucket, for the tests that do not care about keying.
fn shared() -> Shared {
    Shared("everyone".into())
}

// --- One quota ------------------------------------------------------------

/// The quota is spent by the requests that were served, and no further.
#[tokio::test]
async fn requests_within_the_quota_are_served_and_the_rest_refused() {
    let service = limited(Quotas::new(shared(), Counters::default()).quota(Quota::new(
        "fixed",
        3,
        Duration::from_secs(600),
    )));

    for expected_remaining in ["2", "1", "0"] {
        let reply = get(&service, "/counted").call().await;
        assert_eq!(reply.status, StatusCode::NO_CONTENT);
        assert_eq!(
            reply.field("x-ratelimit-remaining").as_deref(),
            Some(expected_remaining)
        );
    }

    let refused = get(&service, "/counted").call().await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused.field("x-ratelimit-remaining").as_deref(), Some("0"));
    assert!(refused.field("retry-after").is_some());
}

/// A refused request does not spend quota.
///
/// The reason the algorithm reads before it increments. If a denial counted,
/// a client that kept retrying would push its own window along and the
/// estimate would never fall — a throttled caller could never recover.
#[tokio::test]
async fn a_refused_request_does_not_spend_the_quota_it_was_refused_by() {
    let counters = Counters::default();
    let service = limited(Quotas::new(shared(), counters.clone()).quota(Quota::new(
        "fixed",
        1,
        Duration::from_secs(600),
    )));

    assert_eq!(
        get(&service, "/counted").call().await.status,
        StatusCode::NO_CONTENT
    );

    let counted_after_success = counters.0.lock().expect("no panic").values().sum::<u64>();

    for _ in 0..5 {
        assert_eq!(
            get(&service, "/counted").call().await.status,
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    assert_eq!(
        counters.0.lock().expect("no panic").values().sum::<u64>(),
        counted_after_success,
        "five refusals spent quota they were refused by"
    );
}

/// Burst raises what the service honours, and says so.
///
/// Advertising the sustained rate while honouring more would be a promise the
/// service breaks in the client's favour and then contradicts in its own
/// headers.
#[tokio::test]
async fn a_burst_allowance_is_both_honoured_and_advertised() {
    let service = limited(
        Quotas::new(shared(), Counters::default())
            .quota(Quota::new("fixed", 2, Duration::from_secs(600)).burst(3)),
    );

    let first = get(&service, "/counted").call().await;
    assert_eq!(first.field("x-ratelimit-limit").as_deref(), Some("5"));

    // Four more inside the ceiling of five.
    for _ in 0..4 {
        assert_eq!(
            get(&service, "/counted").call().await.status,
            StatusCode::NO_CONTENT
        );
    }

    assert_eq!(
        get(&service, "/counted").call().await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

// --- Several quotas -------------------------------------------------------

/// Two windows, and the tighter one refuses first.
#[tokio::test]
async fn the_tightest_quota_is_the_one_that_refuses() {
    let service = limited(
        Quotas::new(shared(), Counters::default())
            .quota(Quota::new("burst", 2, Duration::from_secs(600)))
            .quota(Quota::new("daily", 1_000, Duration::from_secs(86_400))),
    );

    for _ in 0..2 {
        assert_eq!(
            get(&service, "/counted").call().await.status,
            StatusCode::NO_CONTENT
        );
    }

    assert_eq!(
        get(&service, "/counted").call().await.status,
        StatusCode::TOO_MANY_REQUESTS,
        "the per-request window refuses long before the daily one"
    );
}

/// Both quotas reach the wire under the standard spelling.
///
/// The limitation that motivates it: the `X-` triple has room for one.
#[tokio::test]
async fn the_standard_spelling_reports_both_windows() {
    let service = Router::<()>::new()
        .mount(kynos::routes![counted, other])
        .intercept(
            RateLimit::new(
                Quotas::new(shared(), Counters::default())
                    .quota(Quota::new("burst", 5, Duration::from_secs(60)))
                    .quota(Quota::new("daily", 1_000, Duration::from_secs(86_400))),
            )
            .standard_fields(),
        )
        .build(())
        .expect("a describable router");

    let reply = get(&service, "/counted").call().await;
    let policy = reply
        .field("ratelimit-policy")
        .expect("the policies are advertised");

    assert!(policy.contains(r#""burst";q=5;w=60"#), "{policy}");
    assert!(policy.contains(r#""daily";q=1000;w=86400"#), "{policy}");
}

// --- Keying ---------------------------------------------------------------

/// Two routes under one limiter, keyed by route: separate buckets.
#[tokio::test]
async fn keying_by_route_gives_each_operation_its_own_bucket() {
    let service = Router::<()>::new()
        .mount(kynos::routes![counted, other])
        .intercept(RateLimit::new(
            Quotas::new(ByRoute, Counters::default()).quota(Quota::new(
                "fixed",
                1,
                Duration::from_secs(600),
            )),
        ))
        .build(())
        .expect("a describable router");

    assert_eq!(
        get(&service, "/counted").call().await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&service, "/counted").call().await.status,
        StatusCode::TOO_MANY_REQUESTS
    );

    // The other operation's bucket is its own.
    assert_eq!(
        get(&service, "/other").call().await.status,
        StatusCode::NO_CONTENT,
        "one operation's quota refused a request to another"
    );
}

/// Keyed by a header, so two callers do not share a bucket.
#[tokio::test]
async fn keying_by_a_header_separates_two_callers() {
    let service = Router::<()>::new()
        .mount(kynos::routes![counted, other])
        .intercept(RateLimit::new(
            Quotas::new(
                ByHeader(kynos::http::HeaderName::from_static("x-tenant")),
                Counters::default(),
            )
            .quota(Quota::new("fixed", 1, Duration::from_secs(600))),
        ))
        .build(())
        .expect("a describable router");

    assert_eq!(
        get(&service, "/counted")
            .header("x-tenant", "acme")
            .call()
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&service, "/counted")
            .header("x-tenant", "acme")
            .call()
            .await
            .status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        get(&service, "/counted")
            .header("x-tenant", "other")
            .call()
            .await
            .status,
        StatusCode::NO_CONTENT,
        "one tenant's quota refused another's request"
    );
}

/// Two keys joined: per-tenant *and* per-route.
#[tokio::test]
async fn two_keys_joined_partition_by_both() {
    let service = Router::<()>::new()
        .mount(kynos::routes![counted, other])
        .intercept(RateLimit::new(
            Quotas::new(
                And(
                    ByHeader(kynos::http::HeaderName::from_static("x-tenant")),
                    ByRoute,
                ),
                Counters::default(),
            )
            .quota(Quota::new("fixed", 1, Duration::from_secs(600))),
        ))
        .build(())
        .expect("a describable router");

    let call = |path: &'static str, tenant: &'static str| {
        let service = &service;
        async move {
            get(service, path)
                .header("x-tenant", tenant)
                .call()
                .await
                .status
        }
    };

    assert_eq!(call("/counted", "acme").await, StatusCode::NO_CONTENT);
    assert_eq!(
        call("/counted", "acme").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // A different route, same tenant.
    assert_eq!(call("/other", "acme").await, StatusCode::NO_CONTENT);
    // The same route, different tenant.
    assert_eq!(call("/counted", "other").await, StatusCode::NO_CONTENT);
}

/// A key that exempts a request reads no counter and reports the full quota.
#[tokio::test]
async fn an_exempt_request_spends_nothing_and_is_told_so() {
    let counters = Counters::default();
    let service = Router::<()>::new()
        .mount(kynos::routes![counted, other])
        .intercept(RateLimit::new(
            Quotas::new(
                |_: &kynos::http::Request, _: kynos::router::operation::Route<'_>, (): &()| None,
                counters.clone(),
            )
            .quota(Quota::new("fixed", 1, Duration::from_secs(600))),
        ))
        .build(())
        .expect("a describable router");

    for _ in 0..5 {
        let reply = get(&service, "/counted").call().await;
        assert_eq!(reply.status, StatusCode::NO_CONTENT);
        assert_eq!(reply.field("x-ratelimit-remaining").as_deref(), Some("1"));
    }

    assert!(
        counters.0.lock().expect("no panic").is_empty(),
        "an exempt request touched the store"
    );
}

// --- When the store cannot answer ----------------------------------------

/// A counter store outage must not become an API outage.
#[tokio::test]
async fn a_store_that_cannot_answer_allows_by_default() {
    let service = limited(Quotas::new(shared(), Broken).quota(Quota::new(
        "fixed",
        1,
        Duration::from_secs(600),
    )));

    for _ in 0..5 {
        assert_eq!(
            get(&service, "/counted").call().await.status,
            StatusCode::NO_CONTENT
        );
    }
}

/// A deployment that would rather refuse can say so, and gets the 429 the
/// limiter already declares.
///
/// Not a 503: a second status here would collide with `Concurrency` on any
/// route carrying both, and `statuses_disjoint` would refuse to compile it.
#[tokio::test]
async fn a_store_that_cannot_answer_refuses_where_that_was_selected() {
    let service = limited(
        Quotas::new(shared(), Broken)
            .on_store_failure(StoreFailure::Deny)
            .quota(Quota::new("fixed", 1, Duration::from_secs(600))),
    );

    assert_eq!(
        get(&service, "/counted").call().await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

/// Behind a proxy, every client shares the peer address — so a per-IP quota
/// keyed on it is silently a global one.
///
/// The defect `ByClientAddress` exists for. Both requests arrive on the same
/// (absent) socket and carry different `Forwarded` clients; with the key that
/// reads the socket they share a bucket.
#[tokio::test]
async fn keying_by_peer_address_puts_every_proxied_client_in_one_bucket() {
    let service = Router::<()>::new()
        .mount(kynos::routes![counted, other])
        .trusted_proxies(TrustedProxies::hops(1))
        .intercept(RateLimit::new(
            Quotas::new(ByPeerAddress, Counters::default()).quota(Quota::new(
                "fixed",
                1,
                Duration::from_secs(600),
            )),
        ))
        .build(())
        .expect("a describable router");

    assert_eq!(
        get(&service, "/counted")
            .header("forwarded", "for=203.0.113.7")
            .call()
            .await
            .status,
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        get(&service, "/counted")
            .header("forwarded", "for=198.51.100.9")
            .call()
            .await
            .status,
        StatusCode::TOO_MANY_REQUESTS,
        "a different client was refused by the first client's quota, which is what \
         keying on the peer address does behind a proxy"
    );
}

/// The same two requests, keyed by the resolved client address.
#[tokio::test]
async fn keying_by_client_address_separates_two_clients_behind_one_proxy() {
    let service = Router::<()>::new()
        .mount(kynos::routes![counted, other])
        .trusted_proxies(TrustedProxies::hops(1))
        .intercept(RateLimit::new(
            Quotas::new(ByClientAddress, Counters::default()).quota(Quota::new(
                "fixed",
                1,
                Duration::from_secs(600),
            )),
        ))
        .build(())
        .expect("a describable router");

    assert_eq!(
        get(&service, "/counted")
            .header("forwarded", "for=203.0.113.7")
            .call()
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&service, "/counted")
            .header("forwarded", "for=203.0.113.7")
            .call()
            .await
            .status,
        StatusCode::TOO_MANY_REQUESTS,
        "the same client was not counted twice"
    );
    assert_eq!(
        get(&service, "/counted")
            .header("forwarded", "for=198.51.100.9")
            .call()
            .await
            .status,
        StatusCode::NO_CONTENT,
        "one client's quota refused another's request"
    );
}

/// Without a trust policy, `ByClientAddress` believes nothing.
///
/// The safe default made visible: an unconfigured service must not let a client
/// pick the bucket it counts against by writing its own `Forwarded`.
#[tokio::test]
async fn an_unconfigured_service_ignores_the_client_address_a_request_claims() {
    let service = Router::<()>::new()
        .mount(kynos::routes![counted, other])
        .intercept(RateLimit::new(
            Quotas::new(ByClientAddress, Counters::default()).quota(Quota::new(
                "fixed",
                1,
                Duration::from_secs(600),
            )),
        ))
        .build(())
        .expect("a describable router");

    assert_eq!(
        get(&service, "/counted")
            .header("forwarded", "for=203.0.113.7")
            .call()
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&service, "/counted")
            .header("forwarded", "for=198.51.100.9")
            .call()
            .await
            .status,
        StatusCode::TOO_MANY_REQUESTS,
        "a client chose its own bucket by writing a header nobody was trusted to send"
    );
}
