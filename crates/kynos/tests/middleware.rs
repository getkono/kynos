//! Every interceptor Kynos ships, doing what it declares.
//!
//! One reason: an interceptor's declaration and its behaviour are the same text
//! by construction, but *that the text is right* is not something the compiler
//! can check. These drive a built service and read what came back.

#![cfg(all(feature = "macros", feature = "json"))]

use kynos::{
    http::{Method, Request, StatusCode, header},
    middleware::rate_limit::{
        Decision, QuotaPolicy, QuotaUnit, RateLimit, RateLimitPolicy, ServiceLimit,
    },
    router::operation::Route,
};

#[path = "support/mod.rs"]
mod support;

use support::{App, get, send};

/// The one policy both fixtures advertise.
fn advertised() -> Vec<QuotaPolicy> {
    vec![QuotaPolicy {
        name: "default".into(),
        quota: 100,
        window: Some(std::time::Duration::from_secs(60)),
        unit: QuotaUnit::Requests,
    }]
}

/// A policy that always allows, reporting a fixed remaining count and reset.
#[derive(Clone, Debug)]
struct AlwaysAllows(Vec<QuotaPolicy>);

impl AlwaysAllows {
    fn new() -> Self {
        Self(advertised())
    }
}

impl RateLimitPolicy<App> for AlwaysAllows {
    fn advertised(&self) -> &[QuotaPolicy] {
        &self.0
    }

    async fn check(&self, _: &Request, _: Route<'_>, _: &App) -> Decision {
        Decision::allow(ServiceLimit {
            name: "default".into(),
            quota: 100,
            remaining: 97,
            reset: std::time::Duration::from_secs(42),
        })
    }
}

/// A policy that always denies.
#[derive(Clone, Debug)]
struct AlwaysDenies(Vec<QuotaPolicy>);

impl AlwaysDenies {
    fn new() -> Self {
        Self(advertised())
    }
}

impl RateLimitPolicy<App> for AlwaysDenies {
    fn advertised(&self) -> &[QuotaPolicy] {
        &self.0
    }

    async fn check(&self, _: &Request, _: Route<'_>, _: &App) -> Decision {
        Decision::deny(
            std::time::Duration::from_secs(30),
            ServiceLimit {
                name: "default".into(),
                quota: 100,
                remaining: 0,
                reset: std::time::Duration::from_secs(30),
            },
        )
    }
}

/// The module doc promised `RateLimit-*` headers and `Adds` was `()`, so there
/// was no header the interceptor could set — a declaration that said one thing
/// and did another, which is the failure this whole design exists to prevent.
#[tokio::test]
async fn a_rate_limited_service_attaches_the_headers_its_declaration_names() {
    let service = support::router()
        .intercept(RateLimit::new(AlwaysAllows::new()))
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
        .intercept(RateLimit::new(AlwaysDenies::new()))
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
        .intercept(RateLimit::new(AlwaysAllows::new()))
        .openapi()
        .expect("a describable router");

    let emitted = serde_json::to_string(&document).expect("a serializable document");

    assert!(emitted.contains("X-RateLimit-Limit"), "{emitted}");
    assert!(emitted.contains("X-RateLimit-Remaining"), "{emitted}");
    assert!(emitted.contains("X-RateLimit-Reset"), "{emitted}");
}

/// The standard spelling reports every quota, which is why it exists.
///
/// The `X-` triple has room for one, so a limiter enforcing a per-second and a
/// per-day window can only report half of what it enforced. Opting in is a
/// type-state rather than a flag, because it changes what every covered
/// operation declares.
#[tokio::test]
async fn the_standard_spelling_reports_what_the_legacy_triple_cannot() {
    let service = support::router()
        .intercept(RateLimit::new(AlwaysAllows::new()).standard_fields())
        .build(App::new())
        .expect("a describable router");

    let reply = send(&service, Method::DELETE, "/users/1").call().await;

    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert_eq!(
        reply.field("ratelimit").as_deref(),
        Some(r#""default";r=97;t=42"#)
    );
    assert_eq!(
        reply.field("ratelimit-policy").as_deref(),
        Some(r#""default";q=100;w=60"#)
    );

    // And the other spelling is absent, because a response carrying both is two
    // statements of one fact.
    assert!(reply.field("x-ratelimit-limit").is_none());
}

/// A refusal in the standard spelling carries the same fields.
#[tokio::test]
async fn a_standard_spelling_refusal_reports_every_quota_too() {
    let service = support::router()
        .intercept(RateLimit::new(AlwaysDenies::new()).standard_fields())
        .build(App::new())
        .expect("a describable router");

    let reply = get(&service, "/users/1").call().await;

    assert_eq!(reply.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        reply.field(header::RETRY_AFTER.as_str()).as_deref(),
        Some("30")
    );
    assert_eq!(
        reply.field("ratelimit").as_deref(),
        Some(r#""default";r=0;t=30"#)
    );
    assert_eq!(
        reply.field("ratelimit-policy").as_deref(),
        Some(r#""default";q=100;w=60"#)
    );
}

/// The standard spelling declares its own field names, not the `X-` ones.
#[test]
fn the_description_carries_whichever_spelling_was_selected() {
    let emitted = serde_json::to_string(
        &support::router()
            .intercept(RateLimit::new(AlwaysAllows::new()).standard_fields())
            .openapi()
            .expect("a describable router"),
    )
    .expect("a serializable document");

    assert!(emitted.contains("RateLimit-Policy"), "{emitted}");
    assert!(!emitted.contains("X-RateLimit-Limit"), "{emitted}");
}

/// Compression must not re-encode a partial representation.
///
/// RFC 9110 section 14.1.2: when a content coding is applied, *each byte range
/// is calculated with respect to the encoded sequence of bytes*. A 206 whose
/// `Content-Range` names offsets into the identity octets and whose body is
/// gzip is therefore a response whose field is wrong about its own content —
/// and section 14.4 says the recipient of an invalid `Content-Range` MUST NOT
/// recombine it with a stored representation, which is exactly the corruption
/// that follows when a client does.
#[cfg(feature = "compression")]
mod partial {
    use kynos::{
        Router,
        error::rejection::RangeRejection,
        extract::{body::binary::Binary, media::OctetStream},
        http::{StatusCode, header},
        middleware::compression::Compression,
        response::range::{Range, Ranged},
        router::service::Service,
    };

    use super::support::{App, get};

    /// Long enough and repetitive enough that gzip is worth applying, so the
    /// control below fails if compression is simply not running.
    fn octets() -> Vec<u8> {
        b"0123456789".repeat(64)
    }

    #[kynos::get("/recordings/current")]
    async fn recording(
        range: Range<Binary<OctetStream>>,
    ) -> Result<Ranged<Binary<OctetStream>>, RangeRejection> {
        range.apply(Binary::new(octets()))
    }

    fn service() -> Service<App> {
        Router::<App>::new()
            .mount(kynos::routes![recording])
            .intercept(Compression::new())
            .build(App::new())
            .expect("a describable router")
    }

    /// The 206 reaches the wire as the octets its `Content-Range` names.
    #[tokio::test]
    async fn a_partial_representation_is_never_compressed() {
        let service = service();
        let reply = get(&service, "/recordings/current")
            .header("accept-encoding", "gzip")
            .header("range", "bytes=0-99")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            reply.field(header::CONTENT_RANGE.as_str()).as_deref(),
            Some("bytes 0-99/640")
        );
        assert_eq!(reply.field(header::CONTENT_ENCODING.as_str()), None);
        assert_eq!(reply.body.len(), 100);
        assert_eq!(reply.body, octets()[..100]);
    }

    /// The 416 is not compressed either: its `Content-Range` describes the
    /// representation, and a coding applied here would say the same untruth
    /// about a body that is a problem document rather than a part.
    #[tokio::test]
    async fn an_unsatisfiable_range_is_never_compressed() {
        let service = service();
        let reply = get(&service, "/recordings/current")
            .header("accept-encoding", "gzip")
            .header("range", "bytes=99999-")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            reply.field(header::CONTENT_RANGE.as_str()).as_deref(),
            Some("bytes */640")
        );
        assert_eq!(reply.field(header::CONTENT_ENCODING.as_str()), None);
    }

    /// The pass control. Without it the two above hold just as well against a
    /// compression interceptor that has stopped encoding anything at all.
    #[tokio::test]
    async fn a_whole_representation_is_still_compressed() {
        let service = service();
        let reply = get(&service, "/recordings/current")
            .header("accept-encoding", "gzip")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.field(header::ACCEPT_RANGES.as_str()).as_deref(),
            Some("bytes")
        );
        assert_eq!(
            reply.field(header::CONTENT_ENCODING.as_str()).as_deref(),
            Some("gzip")
        );
        assert!(reply.body.len() < octets().len());
    }
}
