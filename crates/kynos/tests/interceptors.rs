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

/// Every interceptor Kynos ships, named against the set this suite accounts
/// for.
///
/// Witnessing a set someone chose says nothing about whether the set is the
/// whole set. The declared side is therefore read off disk — every `.rs` file
/// under `src/middleware/`, walked rather than transcribed — so an interceptor
/// added in a module no list mentions still fails the build.
///
/// Naming the types rather than counting them is what makes the failure
/// readable: a count says two numbers differ, a set says which interceptor
/// nothing accounts for. It is also what lets two branches each add one and
/// merge, since alphabetical insertion puts them on different lines.
///
/// Neither side is `#[cfg]`-gated any more. Both are source text, and a file
/// exists on disk whether or not the feature that compiles it is on, so this
/// now holds at baseline features as well as under `--all-features`.
#[test]
fn every_interceptor_kynos_ships_is_accounted_for() {
    /// Sorted. `Trace` is an `Observer` rather than an `Interceptor` — it
    /// declares nothing, so it is not in this set and is named below instead.
    const WITNESSED: &[&str] = &[
        "BodySize",
        "BodyTimeout",
        "Cache",
        "Compression",
        "Concurrency",
        "Conditional",
        "Cors",
        "Csrf",
        "Decompression",
        "RateLimit",
        "RequestId",
        "SetCookies",
        "Timeout",
    ];

    let declared = implementors_of("> Interceptor<C> for ");

    assert_eq!(
        declared,
        WITNESSED
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "`middleware/` implements `Interceptor` for a different set than this suite accounts \
         for; an interceptor added without a case is one whose declaration nothing reads"
    );
}

/// Every observer Kynos ships, named the same way.
///
/// A separate set because an observer declares *nothing*: it cannot add a
/// header or short-circuit, which is exactly why `Trace` needs no
/// header-versus-declaration case and does need to be accounted for somewhere.
///
/// The list this replaced opened ten files and `compression.rs` was not among
/// them, so an `Observer` implemented there would have been counted by nothing.
/// Walking the directory is what closes that.
#[test]
fn every_observer_kynos_ships_is_accounted_for() {
    /// Sorted.
    const WITNESSED: &[&str] = &["Trace"];

    let declared = implementors_of("> Observer<C> for ");

    assert_eq!(
        declared,
        WITNESSED
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "`middleware/` implements `Observer` for a different set than this suite accounts for"
    );
}

/// The type names `middleware/` implements `marker` for, read off disk.
///
/// `marker` carries the `> ` that closes the impl's generic list, which is what
/// keeps `Interceptor<C>` from also matching `ErasedInterceptor<C>`.
fn implementors_of(marker: &str) -> BTreeSet<String> {
    let mut sources = Vec::new();
    collect_sources(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/middleware")),
        &mut sources,
    );
    assert!(
        !sources.is_empty(),
        "no sources found under `src/middleware/`"
    );

    sources
        .iter()
        .flat_map(|source| {
            source
                .match_indices(marker)
                .map(|(at, _)| {
                    source[at + marker.len()..]
                        .chars()
                        .take_while(|character| character.is_alphanumeric() || *character == '_')
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every `.rs` file under `directory`, read, except a sibling `tests.rs`.
///
/// Test modules are excluded because a fixture implementing `Interceptor` is
/// not something Kynos ships, and the transcribed list this replaced named no
/// `tests.rs` either.
fn collect_sources(directory: &std::path::Path, into: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read `{}`: {error}", directory.display()))
        .map(|entry| entry.expect("read a directory entry").path())
        .collect();
    entries.sort();

    for path in entries {
        if path.is_dir() {
            collect_sources(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path.file_name().is_some_and(|name| name != "tests.rs")
        {
            into.push(
                std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("read `{}`: {error}", path.display())),
            );
        }
    }
}

// --- The short circuits, and the body each owes its description -----------

/// Sorted. `Infallible` is in the set and has no case in the sweep below: it is
/// uninhabited, so there is no value to drive and no status to declare.
const SHORT_CIRCUITS: &[&str] = &[
    "AtCapacity",
    "BodySizeExceeded",
    "CrossSite",
    "Infallible",
    "NotAcceptable",
    "NotModified",
    "RateLimited",
    "RateLimitedFields",
    "TimedOut",
    "Undecodable",
];

/// The members of `SHORT_CIRCUITS` the sweep cannot drive, and why.
const UNCONSTRUCTIBLE: &[&str] = &["Infallible"];

/// Every short circuit Kynos ships, named against the set the sweep drives.
///
/// The same argument as the two sets above, one trait further out: without it a
/// ninth short circuit is a type the sweep silently does not reach. The walk is
/// over the whole crate rather than `src/middleware/`, because `Infallible`'s
/// implementation is in `src/response/mod.rs`.
#[test]
fn every_short_circuit_kynos_ships_is_accounted_for() {
    let declared = impls_of("ShortCircuit");

    assert_eq!(
        declared,
        SHORT_CIRCUITS
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<BTreeSet<_>>(),
        "`src/` implements `ShortCircuit` for a different set than the sweep accounts for; a \
         short circuit added without a case is one whose description nothing reads"
    );
}

/// The type names `crates/kynos/src` implements `trait_name` for, read off disk.
///
/// Line-oriented rather than the `> Marker for ` substring `implementors_of`
/// uses, for two reasons that marker cannot cover: `ShortCircuit` takes no
/// generic argument to close the match on, and `middleware/limits.rs` writes
/// `impl ShortCircuit for TookTooLong` inside a doc example, which a bare
/// substring would count as a shipped implementation. Requiring the line's code
/// to *begin* with `impl ` excludes the `/// # ` prefix and still admits
/// `impl crate::response::ShortCircuit for NotAcceptable`.
fn impls_of(trait_name: &str) -> BTreeSet<String> {
    let mut sources = Vec::new();
    collect_sources(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")),
        &mut sources,
    );
    assert!(!sources.is_empty(), "no sources found under `src/`");

    let marker = format!("{trait_name} for ");

    sources
        .iter()
        .flat_map(|source| source.lines())
        .filter_map(|line| {
            let code = line.trim_start();
            if !code.starts_with("impl ") {
                return None;
            }
            let at = code.find(&marker)?;
            Some(
                code[at + marker.len()..]
                    .chars()
                    .take_while(|character| character.is_alphanumeric() || *character == '_')
                    .collect::<String>(),
            )
        })
        .collect()
}

/// One short circuit's wire response, beside the description it declared.
struct Case {
    name: &'static str,
    /// The statuses the implementation claims, so the sweep can assert it drove
    /// every one of them rather than whichever variant it happened to name.
    claimed: &'static [u16],
    /// The status this value answered with. Read from the wire rather than from
    /// `STATUSES`, because a `STATUSES` list may hold several and one value
    /// answers with exactly one — `Undecodable` declares three and needs three
    /// variants driven to reach them.
    status: u16,
    media_type: Option<String>,
    body_len: usize,
    declared: kynos::openapi::Responses,
}

/// Drives one short circuit and records both halves of its promise.
async fn case<S: kynos::response::ShortCircuit>(
    registry: &mut kynos::schema::registry::Registry,
    value: S,
) -> Case {
    use http_body_util::BodyExt;

    // `responses` and `into_response` resolve through `ShortCircuit`'s
    // supertraits, so neither trait is imported here.
    let declared = S::responses(registry);
    let (parts, body) = value.into_response().into_parts();

    let media_type = parts
        .headers
        .get(kynos::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let (media_type, _) = value.split_once(';').unwrap_or((value, ""));
            media_type.trim().to_ascii_lowercase()
        });

    let body = body.collect().await.expect("a readable body").to_bytes();

    Case {
        name: std::any::type_name::<S>()
            .rsplit("::")
            .next()
            .expect("a type name has a last segment"),
        claimed: S::STATUSES,
        status: parts.status.as_u16(),
        media_type,
        body_len: body.len(),
        declared,
    }
}

/// One value per short circuit this build compiled, driven against one registry.
///
/// Values rather than types, because a description is a claim about what the
/// wire carries and only a value produces one. `Undecodable` appears three
/// times: it declares three statuses and one value answers with one of them.
async fn every_case() -> Vec<Case> {
    use std::time::Duration;

    use kynos::middleware::{
        csrf::CrossSite,
        limits::{AtCapacity, BodySizeExceeded, TimedOut},
        rate_limit::headers::{RateLimited, RateLimitedFields},
    };

    let registry = &mut kynos::schema::registry::Registry::new();
    let mut cases = Vec::new();

    cases.push(case(registry, BodySizeExceeded { limit: 64 }).await);
    cases.push(
        case(
            registry,
            TimedOut {
                after: Duration::from_secs(1),
            },
        )
        .await,
    );
    cases.push(
        case(
            registry,
            AtCapacity {
                retry_after: Some(Duration::from_secs(1)),
            },
        )
        .await,
    );
    cases.push(case(registry, CrossSite).await);
    cases.push(
        case(
            registry,
            RateLimited {
                retry_after: Duration::from_secs(1),
                limit: 10,
            },
        )
        .await,
    );
    cases.push(
        case(
            registry,
            RateLimitedFields {
                retry_after: Duration::from_secs(1),
                limits: Vec::new(),
                policies: Vec::new(),
            },
        )
        .await,
    );

    #[cfg(feature = "compression")]
    {
        use kynos::middleware::{compression::NotAcceptable, decompression::Undecodable};

        cases.push(case(registry, NotAcceptable).await);
        cases.push(case(registry, Undecodable::UnsupportedCoding).await);
        cases.push(case(registry, Undecodable::Malformed).await);
        cases.push(case(registry, Undecodable::TooLarge { limit: 64 }).await);
    }

    #[cfg(feature = "cache")]
    {
        use kynos::{http::HeaderMap, middleware::conditional::NotModified};

        cases.push(case(registry, NotModified::from_headers(&HeaderMap::new())).await);
    }

    cases
}

/// The names the sweep must drive, derived from `SHORT_CIRCUITS` rather than
/// transcribed a second time — so a ninth implementation fails both tests.
///
/// A `cfg` per element rather than a second list, so the set stays derived at
/// every feature combination.
fn expected_names() -> BTreeSet<&'static str> {
    /// Whichever members this build did not compile.
    const ABSENT: &[&str] = &[
        #[cfg(not(feature = "compression"))]
        "NotAcceptable",
        #[cfg(not(feature = "compression"))]
        "Undecodable",
        #[cfg(not(feature = "cache"))]
        "NotModified",
    ];

    SHORT_CIRCUITS
        .iter()
        .copied()
        .filter(|name| !UNCONSTRUCTIBLE.contains(name) && !ABSENT.contains(name))
        .collect()
}

/// Every short circuit's *described* response declares the body its *wire*
/// response sends.
///
/// The defect issue #104 reported, asserted over the whole set rather than at
/// the site that was noticed: eight of the ten implementations put a problem
/// document on the wire under a response declaring no content, and
/// `assert_conformance` read an undeclared content as "nothing to check".
///
/// Both directions. `NotModified` sends no body and must go on declaring none,
/// so this is not "every short circuit declares content" — it is "the
/// declaration and the exchange agree".
#[tokio::test]
async fn every_short_circuit_declares_the_content_it_sends() {
    let cases = every_case().await;

    let driven: BTreeSet<&str> = cases.iter().map(|case| case.name).collect();
    assert_eq!(
        driven,
        expected_names(),
        "a short circuit the sweep does not drive"
    );

    // Every status an implementation claims has a value behind it, so a variant
    // added to a multi-status short circuit cannot go undriven.
    for name in &driven {
        let claimed: BTreeSet<u16> = cases
            .iter()
            .find(|case| &case.name == name)
            .expect("a driven case")
            .claimed
            .iter()
            .copied()
            .collect();
        let reached: BTreeSet<u16> = cases
            .iter()
            .filter(|case| &case.name == name)
            .map(|case| case.status)
            .collect();
        assert_eq!(
            reached, claimed,
            "`{name}` declares statuses the sweep does not drive a value to"
        );
    }

    for case in &cases {
        let key = kynos::openapi::StatusPattern::Code(case.status).to_string();
        let Some(kynos::openapi::RefOr::Item(declared)) = case.declared.responses.get(&key) else {
            panic!(
                "`{}` answers {} and its description declares no such response",
                case.name, case.status
            );
        };

        if let Some(media_type) = case.media_type.as_deref() {
            assert!(
                declared.content.contains_key(media_type),
                "`{}`'s {} sends a `{media_type}` body the description does not declare",
                case.name,
                case.status
            );
        } else {
            assert!(
                case.body_len == 0,
                "`{}`'s {} sends {} bytes with no `Content-Type`",
                case.name,
                case.status,
                case.body_len
            );
            assert!(
                declared.content.is_empty(),
                "`{}`'s {} declares content it does not send",
                case.name,
                case.status
            );
        }
    }
}
