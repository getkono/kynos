//! What the routing path allocates, counted.
//!
//! The allocation-count kind in
//! [`performance.md`](../../../docs/performance.md#the-taxonomy), and the
//! target that document says owns the global allocator. It is a target of its
//! own rather than a sibling `tests.rs` beside the router because a
//! `#[global_allocator]` is process-wide: installed in the library's unit-test
//! binary it would count, and slow, every other unit test in it.
//!
//! **The counters are process-global, so this target is correct only under one
//! process per test.** `stats_alloc` counts into globals rather than into
//! thread locals, so the three tests below would contaminate each other as
//! threads of one binary — under `cargo test` they do, and report figures
//! several times the real ones. `cargo nextest` gives each its own process,
//! which is what [`hermeticity.rs`](hermeticity.rs) exists to hold, and
//! `.config/nextest.toml` is where that is configured. Nothing here can assert
//! it: a test that has already been contaminated cannot notice.
//!
//! **These numbers record a requirement that is not met.**
//! [`nfr.md`](../../../docs/nfr.md#routing) asks for zero allocations on the
//! routing path and the path allocates seven times for a static match. The
//! ceilings are the measurement rather than the target, as
//! [`nfr.md`](../../../docs/nfr.md#thresholds) requires of a first
//! measurement — and this file is the characterization that row points at, so
//! that closing the gap turns something red rather than nothing.

#![cfg(feature = "macros")]

use std::alloc::System;

use kynos::{
    Router,
    extract::params::path::Path,
    http::{Method, Request, body::Body},
    prelude::*,
    response::status::NoContent,
    router::service::Service,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

/// Declared here rather than reached for: `stats_alloc` installs nothing on its
/// own behalf, so this line is the whole of what puts the counter in this
/// binary and in no other.
#[global_allocator]
static ALLOCATOR: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// Every shape measured here, with what it costs today.
///
/// `/users/{id}` is dispatch *and* the `Path` extractor that reads the capture,
/// so its excess over `/ping` is not the router's alone. Splitting the two is
/// the attribution [`nfr.md`](../../../docs/nfr.md#routing) names as the next
/// piece of work.
const SHAPES: [(&str, usize); 3] = [
    // A static match, with no parameter to capture.
    ("/ping", 7),
    // One path parameter, captured and deserialized.
    ("/users/7", 11),
    // A request matching no route at all.
    ("/nope", 6),
];

#[derive(Schema, kynos::PathParams)]
struct One {
    id: u64,
}

/// A handler that allocates nothing, so what a request costs is the router's.
#[kynos::get("/ping")]
async fn ping() -> NoContent {
    NoContent
}

/// The same, behind one path parameter, so a capture is on the measured path.
#[kynos::get("/users/{id}")]
async fn one(Path(path): Path<One>) -> NoContent {
    let _ = path.id;
    NoContent
}

fn service() -> Service<()> {
    Router::<()>::new()
        .mount(kynos::routes![ping, one])
        .build(())
        .expect("a describable router")
}

/// Drives one request and reports the heap operations dispatch made.
///
/// Fresh allocations and reallocations both, so that growing a buffer cannot
/// pass as free. The request is built before the region opens, because parsing
/// a target and boxing a body are the caller's cost rather than the router's,
/// and the response is dropped after the region closes for the same reason.
///
/// There is no warm-up request. `Router::build` initialises eagerly, so the
/// first request through a service costs exactly what the thousandth does —
/// and a warm-up here would be the one construct able to hide a one-time cost
/// introduced later.
async fn counted(service: &Service<()>, target: &str) -> usize {
    let mut request = Request::new(Body::empty());
    *request.method_mut() = Method::GET;
    *request.uri_mut() = target.parse().expect("a usable request target");

    let region = Region::new(ALLOCATOR);
    let response = service.call(request).await;
    let change = region.change();
    let allocations = change.allocations + change.reallocations;

    drop(response);
    allocations
}

/// The record. Named so it reads as one: each ceiling is what the path costs
/// today, and none of them is zero.
#[tokio::test]
async fn the_routing_path_allocates_where_the_requirement_asks_for_nothing() {
    let service = service();

    for (target, ceiling) in SHAPES {
        let counted = counted(&service, target).await;
        assert!(
            counted <= ceiling,
            "{target} allocated {counted} times against a recorded {ceiling}; \
             raising a ceiling is a change to docs/nfr.md, and lowering one is \
             what closing the gap looks like"
        );
    }
}

/// The relation the absolutes are there to hold, and the one that survives a
/// change to any of them: reading a parameter costs more than the static match
/// that found it, and a request that matched nothing costs least of all.
#[tokio::test]
async fn a_capture_is_what_a_path_parameter_costs() {
    let service = service();

    let matched = counted(&service, "/ping").await;
    let captured = counted(&service, "/users/7").await;
    let missed = counted(&service, "/nope").await;

    assert!(
        captured > matched,
        "a captured parameter ({captured}) should cost more than the static \
         match that found it ({matched})"
    );
    assert!(
        missed < matched,
        "a request matching no route ({missed}) should cost less than one that \
         reached a handler ({matched})"
    );
}

/// The leak check, and the half of the requirement that does hold: whatever a
/// request costs, the ten-thousandth costs the same. A count that climbed would
/// be state accumulating on the routing path, which no single-request
/// measurement can see.
///
/// Every shape is replayed, not only the parameterised one: a table that
/// records three numbers and replays one would leave two of them resting on a
/// single reading.
#[tokio::test]
async fn a_replayed_request_costs_what_the_first_one_did() {
    let service = service();

    for (target, _) in SHAPES {
        let first = counted(&service, target).await;
        let mut moved = Vec::new();

        for index in 0..10_000 {
            let counted = counted(&service, target).await;
            if counted != first {
                moved.push((index, counted));
            }
        }

        assert!(
            moved.is_empty(),
            "{target} allocated {first} times on one request and differently \
             on {} of the next ten thousand, starting at {:?}; a count that \
             moves between identical requests is state accumulating on the \
             routing path",
            moved.len(),
            moved.first()
        );
    }
}
