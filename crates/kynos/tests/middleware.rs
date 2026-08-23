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

/// Compression must not re-encode anything a byte range is calculated against.
///
/// Two rules, one reason. RFC 9110 section 14.1.2: when a content coding is
/// applied, *each byte range is calculated with respect to the encoded sequence
/// of bytes*.
///
/// The first rule is about a range already taken. A 206 whose `Content-Range`
/// names offsets into the identity octets and whose body is gzip is a response
/// whose field is wrong about its own content — and section 14.4 says the
/// recipient of an invalid `Content-Range` MUST NOT recombine it with a stored
/// representation, which is exactly the corruption that follows when a client
/// does.
///
/// The second is about a range still to come. A 200 that advertises
/// `Accept-Ranges` invites a later range request, and the offsets that request
/// is answered against are the identity ones — so encoding the 200 while
/// leaving one strong `ETag` over both forms is section 8.8.1's violation, and
/// section 15.3.7.3 then licenses the client to splice the two together.
#[cfg(feature = "compression")]
mod partial {
    use kynos::{
        Router,
        error::rejection::RangeRejection,
        extract::{body::binary::Binary, media::OctetStream},
        http::{StatusCode, header},
        middleware::{compression::Compression, conditional::ETag},
        response::{
            headers::WithHeaders,
            range::{Range, Ranged},
        },
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

    /// The same octets with no range surface over them, which is the one
    /// property the control differs in.
    #[kynos::get("/recordings/transcript")]
    async fn transcript() -> Binary<OctetStream> {
        Binary::new(octets())
    }

    /// The same octets under a *strong* validator.
    #[kynos::get("/recordings/tagged")]
    async fn tagged() -> WithHeaders<Binary<OctetStream>, ETag> {
        WithHeaders::new(Binary::new(octets()), ETag::strong("rev-42"))
    }

    /// The same octets under a handler-stated `Content-Length`.
    ///
    /// The length is what makes the re-encode defect reachable: hyper derives
    /// one from the body when the field is absent, and honours the field when
    /// it is present.
    #[kynos::get("/recordings/measured")]
    async fn measured() -> WithHeaders<Binary<OctetStream>, StatedLength> {
        WithHeaders::new(Binary::new(octets()), StatedLength(octets().len()))
    }

    /// A `Content-Length` a handler attaches to its own response.
    #[derive(Clone, Copy, Debug)]
    struct StatedLength(usize);

    impl kynos::extract::params::header::HeaderParams for StatedLength {
        const NAMES: &'static [&'static str] = &["content-length"];
        const DESCRIBED: bool = false;

        fn encode(&self) -> Vec<(kynos::http::HeaderName, kynos::http::HeaderValue)> {
            vec![(
                header::CONTENT_LENGTH,
                kynos::http::HeaderValue::from_str(&self.0.to_string()).expect("a decimal length"),
            )]
        }
    }

    /// The same octets under a *weak* one, which is the control.
    #[kynos::get("/recordings/weakly-tagged")]
    async fn weakly_tagged() -> WithHeaders<Binary<OctetStream>, ETag> {
        WithHeaders::new(Binary::new(octets()), ETag::weak("rev-42"))
    }

    fn service() -> Service<App> {
        Router::<App>::new()
            .mount(kynos::routes![
                recording,
                transcript,
                tagged,
                weakly_tagged,
                measured
            ])
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

    /// A whole representation that says it ranges is left alone as well.
    ///
    /// The 206 the client asks for next is sliced from the identity octets, so
    /// a 200 encoded here is the other half of the same representation under
    /// the same validator.
    #[tokio::test]
    async fn a_representation_that_advertises_ranges_is_never_compressed() {
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
        assert_eq!(reply.field(header::CONTENT_ENCODING.as_str()), None);
        assert_eq!(reply.body, octets());
    }

    /// The pass control. Without it the three above hold just as well against a
    /// compression interceptor that has stopped encoding anything at all.
    ///
    /// Same octets, same media type, same interceptor: the only difference is
    /// that nothing here advertises a range.
    #[tokio::test]
    async fn a_representation_that_advertises_no_range_is_still_compressed() {
        let service = service();
        let reply = get(&service, "/recordings/transcript")
            .header("accept-encoding", "gzip")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.field(header::ACCEPT_RANGES.as_str()), None);
        assert_eq!(
            reply.field(header::CONTENT_ENCODING.as_str()).as_deref(),
            Some("gzip")
        );
        assert!(reply.body.len() < octets().len());
    }

    /// A strong validator stops the encoder.
    ///
    /// RFC 9110 section 8.8.1: "if the origin server sends the same validator
    /// for a representation with a gzip content coding applied as it does for a
    /// representation with no content coding, then that validator is weak."
    /// Encoding beneath a strong tag makes the tag name two representations,
    /// which is exactly what section 8.8.1 says it may not.
    #[tokio::test]
    async fn a_strongly_tagged_representation_is_never_compressed() {
        let service = service();
        let reply = get(&service, "/recordings/tagged")
            .header("accept-encoding", "gzip")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.field(header::CONTENT_ENCODING.as_str()),
            None,
            "a strong tag was left naming both the identity and the encoded form"
        );
        assert_eq!(reply.body.len(), octets().len());
    }

    /// The length a re-encoded response states is the length it sends.
    ///
    /// RFC 9110 section 8.6: "a sender MUST NOT forward a message with a
    /// Content-Length header field value that is known to be incorrect", and
    /// section 8.4 defines the representation "in terms of the coded form" — so
    /// a length written before encoding names a body that no longer exists.
    ///
    /// The handler here states its own length, which is what makes the case
    /// reachable: without one, hyper derives it from the encoded body and the
    /// defect is invisible.
    #[tokio::test]
    async fn a_re_encoded_response_states_the_length_it_actually_sends() {
        let service = service();
        let reply = get(&service, "/recordings/measured")
            .header("accept-encoding", "gzip")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.field(header::CONTENT_ENCODING.as_str()).as_deref(),
            Some("gzip")
        );

        let stated = reply
            .field(header::CONTENT_LENGTH.as_str())
            .expect("a stated length")
            .parse::<usize>()
            .expect("a decimal length");

        assert_eq!(
            stated,
            reply.body.len(),
            "the stated length names the identity octets, not the ones sent"
        );
        assert!(stated < octets().len());
    }

    /// A weak validator does not, which is the control.
    ///
    /// A weak validator is *defined* as one that may be shared by two
    /// representations, so a response that already says `W/` is telling the
    /// truth after encoding. The pair differs in exactly that prefix.
    #[tokio::test]
    async fn a_weakly_tagged_representation_is_still_compressed() {
        let service = service();
        let reply = get(&service, "/recordings/weakly-tagged")
            .header("accept-encoding", "gzip")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.field(header::CONTENT_ENCODING.as_str()).as_deref(),
            Some("gzip")
        );
        assert!(reply.body.len() < octets().len());
    }
}

/// A served asset is where the unsound splice was reachable end to end.
///
/// [`assets!`](kynos::assets) mints a strong entity tag from the file's
/// contents, and `If-Range` takes the strong comparison — so an encoded 200
/// carrying the identity file's tag is a validator the client is entitled to
/// resume against, and RFC 9110 section 15.3.7.3 then has it append identity
/// octets to an encoded prefix. Nothing reports an error; the file is simply
/// wrong.
///
/// The control for these lives in [`partial`], which drives the same
/// interceptor over a representation that advertises no range.
#[cfg(all(feature = "compression", feature = "assets"))]
mod ranged_assets {
    use kynos::{
        Router,
        http::{StatusCode, header},
        middleware::compression::Compression,
        router::{group::Group, service::Service},
    };

    use super::support::get;

    kynos::assets! {
        /// The same fixture set `tests/assets.rs` serves.
        struct Fixture;
        dir = "tests/assets",
        exclude = [".map"],
    }

    /// The stylesheet, whole.
    const STYLESHEET: &str = "body { margin: 0 }\n";

    fn service() -> Service<()> {
        Router::<()>::new()
            .group(Group::new("/static").mount(Fixture::assets()))
            .intercept(Compression::new())
            .build(())
            .expect("a describable router")
    }

    /// The tag a client stores names the octets a later range is cut from.
    ///
    /// Asserted as one exchange pair rather than two tests, because the defect
    /// is the two disagreeing: either half alone is correct on its own terms.
    #[tokio::test]
    async fn a_resumed_asset_download_is_spliced_from_the_representation_its_tag_named() {
        let service = service();

        let whole = get(&service, "/static/css/app.css")
            .header("accept-encoding", "gzip")
            .call()
            .await;

        assert_eq!(whole.status, StatusCode::OK);
        assert_eq!(whole.field(header::CONTENT_ENCODING.as_str()), None);
        assert_eq!(whole.text(), STYLESHEET);

        let etag = whole
            .field(header::ETAG.as_str())
            .expect("an entity tag over the octets that were sent");

        // What a client that lost the connection after six bytes sends next.
        let resumed = get(&service, "/static/css/app.css")
            .header("accept-encoding", "gzip")
            .header("if-range", &etag)
            .header("range", "bytes=6-")
            .call()
            .await;

        assert_eq!(resumed.status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resumed.field(header::CONTENT_RANGE.as_str()).as_deref(),
            Some("bytes 6-18/19")
        );
        assert_eq!(resumed.field(header::CONTENT_ENCODING.as_str()), None);

        // The splice the client performs, which must reproduce the file.
        let mut spliced = whole.text()[..6].to_owned();
        spliced.push_str(&resumed.text());
        assert_eq!(spliced, STYLESHEET);
    }
}

/// A configured compression level reaching the encoder.
///
/// The failure this rules out is silent and total: a builder that stores a
/// level and an encoder constructed without it compile perfectly, serve
/// perfectly, and differ from a correct implementation only in a number nobody
/// looks at. `Compression::min_size` had that shape once. So these compare two
/// services that differ in exactly the level, and assert the bytes differ.
#[cfg(feature = "compression")]
mod levels {
    use kynos::{
        Router,
        extract::{body::binary::Binary, media::OctetStream},
        http::{StatusCode, header},
        middleware::compression::{
            Compression,
            levels::{BrotliLevel, GzipLevel, ZstdLevel},
        },
        router::service::Service,
    };

    use super::support::{App, get};

    /// Long enough that the level makes a measurable difference, and structured
    /// enough that a stronger search finds more than a weaker one.
    ///
    /// A constant repeat would compress to nearly nothing at every level and
    /// the sizes would coincide, which would make the assertions below pass for
    /// a level that was never applied.
    fn octets() -> Vec<u8> {
        (0..8_192_u32).fold(Vec::new(), |mut octets, index| {
            octets.extend_from_slice(
                format!("{index:x} the quick brown fox {}\n", index % 97).as_bytes(),
            );
            octets
        })
    }

    #[kynos::get("/report")]
    async fn report() -> Binary<OctetStream> {
        Binary::new(octets())
    }

    fn service(compression: Compression) -> Service<App> {
        Router::<App>::new()
            .mount(kynos::routes![report])
            .intercept(compression)
            .build(App::new())
            .expect("a describable router")
    }

    /// The encoded length under `compression`, for the coding `accept` asks for.
    async fn encoded_length(compression: Compression, accept: &str) -> usize {
        let service = service(compression);
        let reply = get(&service, "/report")
            .header("accept-encoding", accept)
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.field(header::CONTENT_ENCODING.as_str()).as_deref(),
            Some(accept),
            "the fixture was not encoded, so nothing here says anything about levels"
        );

        reply.body.len()
    }

    #[tokio::test]
    async fn a_stronger_gzip_level_sends_fewer_bytes_than_a_weaker_one() {
        let fastest =
            encoded_length(Compression::new().gzip_level(GzipLevel::FASTEST), "gzip").await;
        let best = encoded_length(Compression::new().gzip_level(GzipLevel::BEST), "gzip").await;

        assert!(
            best < fastest,
            "gzip 9 produced {best} bytes and gzip 1 produced {fastest}"
        );
    }

    #[tokio::test]
    async fn a_stronger_brotli_quality_sends_fewer_bytes_than_a_weaker_one() {
        let fastest =
            encoded_length(Compression::new().brotli_level(BrotliLevel::FASTEST), "br").await;
        let best = encoded_length(Compression::new().brotli_level(BrotliLevel::BEST), "br").await;

        assert!(
            best < fastest,
            "brotli 11 produced {best} bytes and brotli 1 produced {fastest}"
        );
    }

    #[tokio::test]
    async fn a_stronger_zstd_level_sends_fewer_bytes_than_a_weaker_one() {
        let fastest =
            encoded_length(Compression::new().zstd_level(ZstdLevel::FASTEST), "zstd").await;
        let best = encoded_length(Compression::new().zstd_level(ZstdLevel::BEST), "zstd").await;

        assert!(
            best < fastest,
            "zstd 22 produced {best} bytes and zstd 1 produced {fastest}"
        );
    }

    /// The control the three above need: an unconfigured `Compression` still
    /// encodes, and encodes to something between the extremes. Without it they
    /// pass for a builder whose levels are the only thing that ever worked.
    #[tokio::test]
    async fn the_default_level_sits_between_the_two_extremes() {
        let fastest =
            encoded_length(Compression::new().gzip_level(GzipLevel::FASTEST), "gzip").await;
        let default = encoded_length(Compression::new(), "gzip").await;
        let best = encoded_length(Compression::new().gzip_level(GzipLevel::BEST), "gzip").await;

        assert!(
            best <= default && default < fastest,
            "gzip 1 produced {fastest}, the default {default}, and gzip 9 {best}"
        );
    }
}

/// A handler overruling negotiation for one response.
///
/// Negotiation decides *which* coding. This decides whether the question is
/// asked at all, and it is a property of the response rather than of the route
/// because both reasons for setting it are: a body that reflects a secret, and
/// a body too large to be worth sending as it is.
#[cfg(feature = "compression")]
mod encoding_policy {
    use kynos::{
        Router,
        extract::{body::binary::Binary, media::OctetStream},
        http::{StatusCode, header},
        middleware::compression::{
            Compression,
            policy::{Encoding, WithEncoding},
        },
        router::service::Service,
    };

    use super::support::{App, get};

    fn octets() -> Vec<u8> {
        b"0123456789".repeat(256)
    }

    /// Reflects attacker-chosen input beside something secret, which is the
    /// shape BREACH needs. Never encoded, whatever the client accepts.
    #[kynos::get("/confirm")]
    async fn confirm() -> WithEncoding<Binary<OctetStream>> {
        WithEncoding::new(Binary::new(octets()), Encoding::Disabled)
    }

    /// Too large to be worth sending as it is, so identity stops being an
    /// answer.
    #[kynos::get("/export")]
    async fn export() -> WithEncoding<Binary<OctetStream>> {
        WithEncoding::new(Binary::new(octets()), Encoding::Required)
    }

    /// The control: the same octets, saying nothing.
    #[kynos::get("/report")]
    async fn report() -> Binary<OctetStream> {
        Binary::new(octets())
    }

    fn service() -> Service<App> {
        Router::<App>::new()
            .mount(kynos::routes![confirm, export, report])
            .intercept(Compression::new())
            .build(App::new())
            .expect("a describable router")
    }

    #[tokio::test]
    async fn a_response_that_refuses_encoding_is_not_encoded() {
        let service = service();
        let reply = get(&service, "/confirm")
            .header("accept-encoding", "gzip, br, zstd")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.field(header::CONTENT_ENCODING.as_str()), None);
        assert_eq!(reply.body, octets());
    }

    /// The control for the case above, differing in exactly the policy: the
    /// same octets, the same request, no refusal. Without it that case passes
    /// for a service where compression is simply not running.
    #[tokio::test]
    async fn a_response_that_says_nothing_is_encoded_as_usual() {
        let service = service();
        let reply = get(&service, "/report")
            .header("accept-encoding", "gzip")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.field(header::CONTENT_ENCODING.as_str()).as_deref(),
            Some("gzip")
        );
    }

    #[tokio::test]
    async fn a_response_that_requires_encoding_is_encoded() {
        let service = service();
        let reply = get(&service, "/export")
            .header("accept-encoding", "gzip")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.field(header::CONTENT_ENCODING.as_str()).as_deref(),
            Some("gzip")
        );
    }

    /// The point of `Required`. A client that will take only identity is told
    /// no, rather than handed the whole representation uncompressed.
    #[tokio::test]
    async fn a_client_that_will_take_only_identity_is_refused_what_requires_encoding() {
        let service = service();
        let reply = get(&service, "/export")
            .header("accept-encoding", "identity")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::NOT_ACCEPTABLE);
    }

    /// The control for it, and the one that shows `Required` is a per-response
    /// decision rather than something the interceptor does to every route: the
    /// same client, the same service, a response that did not ask.
    #[tokio::test]
    async fn the_same_client_is_served_a_response_that_did_not_require_encoding() {
        let service = service();
        let reply = get(&service, "/report")
            .header("accept-encoding", "identity")
            .call()
            .await;

        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.field(header::CONTENT_ENCODING.as_str()), None);
        assert_eq!(reply.body, octets());
    }
}
