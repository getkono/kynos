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
        "Cache",
        "Compression",
        "Concurrency",
        "Conditional",
        "Cors",
        "Csrf",
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
