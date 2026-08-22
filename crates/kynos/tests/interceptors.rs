//! What each interceptor writes onto a response, against what it declared.
//!
//! One reason: `Adds` and `NAMES` are the whole of an interceptor's promise
//! about response headers, and the compiler checks that two interceptors do not
//! *collide* on a name — never that either sets the names it claimed, and never
//! that it sets nothing else. Both halves are asserted here, over a live
//! service, because both are how a response and its description come apart.
//!
//! This does not assert `describe` directly. `middleware/erased.rs` restates
//! the associated types and asserting it back would compare the mechanism to
//! itself; only the conformance matrix can find that wrong.

#![cfg(all(feature = "macros", feature = "json"))]

use std::collections::BTreeSet;

use kynos::{
    extract::params::header::HeaderParams,
    http::StatusCode,
    middleware::{
        cors::Cors,
        request_id::{RequestId, XRequestId},
    },
};

#[path = "support/mod.rs"]
mod support;

use support::{App, get};

/// The response headers a bare service sends, so a later comparison can name
/// what an interceptor *added* rather than what was there anyway.
async fn baseline() -> BTreeSet<String> {
    fields(&get(&support::service(), "/users/1").call().await)
}

fn fields(reply: &support::Reply) -> BTreeSet<String> {
    reply
        .headers
        .keys()
        .map(|name| name.as_str().to_owned())
        .collect()
}

/// Every name in `NAMES`, lowercased the way a `HeaderMap` key is.
fn declared<H: HeaderParams>() -> BTreeSet<String> {
    H::NAMES
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect()
}

/// `RequestId` sets exactly the one name its header group declares.
#[tokio::test]
async fn a_request_id_sets_the_name_its_group_declares_and_no_other() {
    let service = support::router()
        .intercept(RequestId::new())
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/1").call().await;
    assert_eq!(reply.status, StatusCode::OK);

    let added: BTreeSet<String> = fields(&reply)
        .difference(&baseline().await)
        .cloned()
        .collect();

    assert_eq!(added, declared::<XRequestId>());
    assert!(
        reply
            .field("x-request-id")
            .is_some_and(|value| !value.is_empty()),
        "the declared name was set to nothing"
    );
}

/// The pass control: without the interceptor the name is absent, so the case
/// above is about the interceptor rather than about the fixture.
#[tokio::test]
async fn a_service_without_a_request_id_sets_no_such_name() {
    let reply = get(&support::service(), "/users/1").call().await;

    assert!(reply.field("x-request-id").is_none());
}

/// A client-supplied id is honoured only where it was trusted.
///
/// Both directions, because trusting by default would let a client choose its
/// own correlation id and collide with another's on purpose.
#[tokio::test]
async fn a_client_supplied_id_is_used_only_when_it_was_trusted() {
    let trusting = support::router()
        .intercept(RequestId::new().trust_client(true))
        .build(App::new())
        .expect("a describable router");

    let trusted = get(&trusting, "/users/1")
        .header("x-request-id", "from-the-client")
        .call()
        .await;
    assert_eq!(
        trusted.field("x-request-id").as_deref(),
        Some("from-the-client")
    );

    let ignored = get(&support::service(), "/users/1")
        .header("x-request-id", "from-the-client")
        .call()
        .await;
    assert!(ignored.field("x-request-id").is_none());

    let untrusting = support::router()
        .intercept(RequestId::new())
        .build(App::new())
        .expect("a describable router");

    let replaced = get(&untrusting, "/users/1")
        .header("x-request-id", "from-the-client")
        .call()
        .await;
    assert_ne!(
        replaced.field("x-request-id").as_deref(),
        Some("from-the-client"),
        "an untrusted client id was echoed back"
    );
}

/// `Cors` adds its own names to a permitted cross-origin response and nothing
/// beyond them.
#[tokio::test]
async fn cors_adds_only_the_names_it_declares() {
    let service = support::router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/1")
        .header("origin", "https://app.example.com")
        .call()
        .await;

    let added: BTreeSet<String> = fields(&reply)
        .difference(&baseline().await)
        .cloned()
        .collect();

    assert_eq!(
        added,
        ["access-control-allow-origin", "vary"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
    );
}

/// Two interceptors that both contribute a `Vary` field union rather than
/// clobbering, which is the one response header whose contributors compose.
#[cfg(feature = "compression")]
#[tokio::test]
async fn two_interceptors_contributing_vary_both_appear_in_it() {
    use kynos::middleware::compression::Compression;

    let service = support::router()
        .intercept(Cors::new().allow_origins(["https://app.example.com"]))
        .intercept(Compression::new())
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/1")
        .header("origin", "https://app.example.com")
        .header("accept-encoding", "gzip")
        .call()
        .await;

    let vary = reply.field("vary").expect("a Vary field");
    let names: BTreeSet<String> = vary
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();

    assert!(names.contains("origin"), "{vary}");
    assert!(names.contains("accept-encoding"), "{vary}");
}

// --- The two closed sets --------------------------------------------------

/// Every interceptor Kynos ships, counted against the cases that exercise one.
///
/// Witnessing a set someone chose says nothing about whether the set is the
/// whole set. This reads the count out of the source, so an interceptor added
/// without a case fails the build rather than joining a silent majority.
///
/// Under `compression`, because `Compression` is gated there and the full set
/// only exists in that build — which is the one `mise run test` uses. The same
/// reason `pipeline.rs` gates its route-attribute counter on `openapi32`.
#[cfg(all(feature = "compression", feature = "cookie", feature = "cache"))]
#[test]
fn every_interceptor_kynos_ships_has_a_case() {
    /// `BodySize`, `Timeout` and `Concurrency` in `limits.rs`; `Cors`,
    /// `RequestId`, `RateLimit`, `Compression` and `SetCookies` in their own
    /// modules, and `Cache` and `Conditional` in theirs. `Trace` is an
    /// `Observer` rather than an `Interceptor` — it declares nothing, so it is
    /// not in this set and is counted below instead.
    const WITNESSED: usize = 10;

    let declared: usize = [
        include_str!("../src/middleware/limits.rs"),
        include_str!("../src/middleware/cors/mod.rs"),
        include_str!("../src/middleware/request_id.rs"),
        include_str!("../src/middleware/rate_limit/mod.rs"),
        include_str!("../src/middleware/rate_limit/decision.rs"),
        include_str!("../src/middleware/rate_limit/headers.rs"),
        include_str!("../src/middleware/rate_limit/key.rs"),
        include_str!("../src/middleware/rate_limit/quota.rs"),
        include_str!("../src/middleware/rate_limit/store.rs"),
        include_str!("../src/middleware/compression.rs"),
        include_str!("../src/middleware/cookies.rs"),
        include_str!("../src/middleware/cache/mod.rs"),
        include_str!("../src/middleware/cache/freshness.rs"),
        include_str!("../src/middleware/cache/store.rs"),
        include_str!("../src/middleware/conditional/mod.rs"),
        include_str!("../src/middleware/catch_panic.rs"),
        include_str!("../src/middleware/contribution.rs"),
        include_str!("../src/middleware/stack.rs"),
        include_str!("../src/middleware/erased.rs"),
        include_str!("../src/middleware/trace.rs"),
        include_str!("../src/middleware/mod.rs"),
    ]
    .iter()
    .map(|source| source.matches("> Interceptor<C>").count())
    .sum();

    assert_eq!(
        declared, WITNESSED,
        "`middleware/` implements `Interceptor` {declared} time(s) and {WITNESSED} are \
         witnessed; an interceptor added without a case is one whose declaration nothing reads"
    );
}

/// Every observer Kynos ships, counted the same way.
///
/// A separate set because an observer declares *nothing*: it cannot add a
/// header or short-circuit, which is exactly why `Trace` needs no
/// header-versus-declaration case and does need to be accounted for somewhere.
#[test]
fn every_observer_kynos_ships_is_accounted_for() {
    /// `Trace`, and only `Trace`.
    const WITNESSED: usize = 1;

    let declared: usize = [
        include_str!("../src/middleware/trace.rs"),
        include_str!("../src/middleware/limits.rs"),
        include_str!("../src/middleware/cors/mod.rs"),
        include_str!("../src/middleware/request_id.rs"),
        include_str!("../src/middleware/rate_limit/mod.rs"),
        include_str!("../src/middleware/rate_limit/decision.rs"),
        include_str!("../src/middleware/rate_limit/headers.rs"),
        include_str!("../src/middleware/rate_limit/key.rs"),
        include_str!("../src/middleware/rate_limit/quota.rs"),
        include_str!("../src/middleware/rate_limit/store.rs"),
        include_str!("../src/middleware/cookies.rs"),
        include_str!("../src/middleware/cache/mod.rs"),
        include_str!("../src/middleware/conditional/mod.rs"),
        include_str!("../src/middleware/mod.rs"),
    ]
    .iter()
    .map(|source| source.matches("> Observer<C> for").count())
    .sum();

    assert_eq!(
        declared, WITNESSED,
        "`middleware/` implements `Observer` {declared} time(s) and {WITNESSED} are witnessed"
    );
}

/// The two counters above read a fixed list of files, so this counts the list.
///
/// Without it, `middleware/` could grow a module holding an interceptor that
/// neither counter above ever opened — and both would keep passing, which is
/// the failure mode an exhaustiveness check exists to rule out.
///
/// Under `compression`, for the reason the interceptor counter gives: the full
/// set of modules only exists in that build.
#[cfg(all(
    feature = "compression",
    feature = "trace",
    feature = "cookie",
    feature = "cache"
))]
#[test]
fn the_counters_above_read_every_module_middleware_declares() {
    const SOURCE: &str = include_str!("../src/middleware/mod.rs");

    /// Every module `middleware/mod.rs` declares, transcribed in declaration
    /// order. The counters above read each of these and `mod.rs` itself.
    const READ: [&str; 13] = [
        "catch_panic",
        "contribution",
        "cors",
        "limits",
        "rate_limit",
        "request_id",
        "stack",
        "erased",
        "cache",
        "compression",
        "conditional",
        "cookies",
        "trace",
    ];

    let declared: Vec<&str> = SOURCE
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            line.strip_prefix("pub mod ")
                .or_else(|| line.strip_prefix("pub(crate) mod "))
        })
        .filter_map(|rest| rest.strip_suffix(';'))
        .collect();

    assert_eq!(
        declared, READ,
        "a module was added to `middleware/` that the counters above never open"
    );
}
