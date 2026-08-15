//! Every interceptor Kynos ships, doing what it declares.
//!
//! One reason: an interceptor's declaration and its behaviour are the same text
//! by construction, but *that the text is right* is not something the compiler
//! can check. These drive a built service and read what came back.

#![cfg(all(feature = "macros", feature = "json"))]

use kynos::{
    http::{Method, Request, StatusCode, header},
    middleware::rate_limit::{Decision, RateLimit, RateLimitPolicy},
};

#[path = "support/mod.rs"]
mod support;

use support::{App, get, send};

/// A policy that always allows, reporting a fixed remaining count and reset.
#[derive(Clone, Debug)]
struct AlwaysAllows;

impl RateLimitPolicy<App> for AlwaysAllows {
    async fn check(&self, _: &Request, _: &App) -> Decision {
        Decision::Allow {
            remaining: 97,
            reset: std::time::Duration::from_secs(42),
        }
    }
}

/// A policy that always denies.
#[derive(Clone, Debug)]
struct AlwaysDenies;

impl RateLimitPolicy<App> for AlwaysDenies {
    async fn check(&self, _: &Request, _: &App) -> Decision {
        Decision::Deny {
            retry_after: std::time::Duration::from_secs(30),
        }
    }
}

/// The module doc promised `RateLimit-*` headers and `Adds` was `()`, so there
/// was no header the interceptor could set — a declaration that said one thing
/// and did another, which is the failure this whole design exists to prevent.
#[tokio::test]
async fn a_rate_limited_service_attaches_the_headers_its_declaration_names() {
    let service = support::router()
        .intercept(RateLimit::new(100, AlwaysAllows))
        .build(App::new())
        .expect("a describable router");

    let reply = send(&service, Method::DELETE, "/users/1").call().await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert_eq!(reply.field("x-ratelimit-limit").as_deref(), Some("100"));
    assert_eq!(reply.field("x-ratelimit-remaining").as_deref(), Some("97"));
    assert_eq!(reply.field("x-ratelimit-reset").as_deref(), Some("42"));
}

/// A denial reports no remaining requests, and reuses the delay it already
/// computed rather than asking the policy for a second number.
#[tokio::test]
async fn a_denial_carries_the_headers_its_own_response_type_describes() {
    let service = support::router()
        .intercept(RateLimit::new(100, AlwaysDenies))
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/1").call().await;

    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        reply.field(header::RETRY_AFTER.as_str()).as_deref(),
        Some("30")
    );
    assert_eq!(reply.field("x-ratelimit-limit").as_deref(), Some("100"));
    assert_eq!(reply.field("x-ratelimit-remaining").as_deref(), Some("0"));
    assert_eq!(reply.field("x-ratelimit-reset").as_deref(), Some("30"));
}

/// Setting a header means declaring it, and declaring it means a client
/// generator can see it.
#[test]
fn the_description_carries_the_rate_limit_headers_on_a_success() {
    let document = support::router()
        .intercept(RateLimit::new(100, AlwaysAllows))
        .openapi()
        .expect("a describable router");

    let emitted = serde_json::to_string(&document).expect("a serializable document");

    assert!(emitted.contains("X-RateLimit-Limit"), "{emitted}");
    assert!(emitted.contains("X-RateLimit-Remaining"), "{emitted}");
    assert!(emitted.contains("X-RateLimit-Reset"), "{emitted}");
}
