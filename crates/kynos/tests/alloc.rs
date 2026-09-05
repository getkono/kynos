//! What the routing path allocates, counted.
//!
//! The allocation-count kind in
//! [`performance.md`](../../../docs/performance.md#the-taxonomy), and the
//! target that document says owns the global allocator. It is a target of its
//! own rather than a sibling `tests.rs` beside the router because a
//! `#[global_allocator]` is process-wide: installed in the library's unit-test
//! binary it would count, and slow, every other unit test in it.
//!
//! **The counter is per-thread, and that is what makes a reading the routing
//! path's.** `alloc_counter` counts into thread locals rather than into
//! globals, so a region reports what the measuring thread allocated and
//! nothing else. A process-global counter cannot: `libtest` runs a test on a
//! thread it spawns and keeps its own alive beside it, so a second thread able
//! to allocate inside a region is always there, and one process per test does
//! not make one thread per process. `stats_alloc` was the counter here and is
//! global, which moved a replayed request's count on roughly one request in
//! ten thousand — read, at the time, as state accumulating on the routing
//! path. `work_on_another_thread_is_not_counted` is what holds the counter
//! this target installs — by including
//! [`support/counting.rs`](support/counting.rs) — to being the other kind.
//!
//! One process per test is still the contract this target runs under — see
//! [`hermeticity.rs`](hermeticity.rs) and `.config/nextest.toml` — but the
//! numbers below no longer rest on it.
//!
//! **These numbers record a requirement that is not met.**
//! [`nfr.md`](../../../docs/nfr.md#routing) asks for zero allocations on the
//! routing path and the path allocates seven times for a static match. The
//! ceilings are the measurement rather than the target, as
//! [`nfr.md`](../../../docs/nfr.md#thresholds) requires of a first
//! measurement — and this file is the characterization that row points at, so
//! that closing the gap turns something red rather than nothing.

#![cfg(feature = "macros")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use alloc_counter::count_alloc;
use kynos::{
    Router, extract::params::path::Path, prelude::*, response::status::NoContent,
    router::service::Service,
};

/// The counter and the driver, shared so that a second counting target does
/// not copy them. Including this module is what installs the allocator.
#[path = "support/counting.rs"]
mod counting;

use counting::counted;

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

/// The instrument's own invariant, and the one every number below rests on: a
/// count is what *this* thread allocated, and nothing else.
///
/// The process is never quiet. `libtest` runs a test on a thread it spawns and
/// keeps its own alive beside it, so a second thread able to allocate is always
/// there -- one process per test does not make one thread per process. A
/// counter that adds every thread's work reports that noise as the router's
/// cost, on whichever microsecond-wide region happens to be open when it lands.
///
/// The handshake is two `AtomicBool`s rather than a `Barrier` or a channel,
/// because the region below has to allocate nothing of its own and a spin on an
/// atomic is the only rendezvous that is allocation-free by construction. The
/// other thread's allocation falls strictly between the two, which makes the
/// reading deterministic in both directions: a process-global counter reads it
/// every time, and a per-thread counter never does.
#[test]
fn work_on_another_thread_is_not_counted() {
    let go = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));

    // Spawned before the region opens: boxing the closure and the join packet
    // happens on this thread, and is the caller's cost rather than the
    // measurement's.
    let other = {
        let (go, done) = (Arc::clone(&go), Arc::clone(&done));
        thread::spawn(move || {
            while !go.load(Ordering::Acquire) {
                std::hint::spin_loop();
            }

            drop(std::hint::black_box(Vec::<u8>::with_capacity(1024)));
            done.store(true, Ordering::Release);
        })
    };

    let ((allocations, reallocations, _), ()) = count_alloc(|| {
        go.store(true, Ordering::Release);
        while !done.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
    });

    other.join().expect("the other thread to finish");

    let counted = allocations + reallocations;
    assert_eq!(
        counted, 0,
        "a region open on this thread counted {counted} allocation(s) that \
         another thread made; a count that carries the rest of the process is \
         not a measurement of the routing path"
    );
}

/// The record. Named so it reads as one: each ceiling is what the path costs
/// today, and none of them is zero.
#[test]
fn the_routing_path_allocates_where_the_requirement_asks_for_nothing() {
    let service = service();

    for (target, ceiling) in SHAPES {
        let counted = counted(&service, target);
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
#[test]
fn a_capture_is_what_a_path_parameter_costs() {
    let service = service();

    let matched = counted(&service, "/ping");
    let captured = counted(&service, "/users/7");
    let missed = counted(&service, "/nope");

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
#[test]
fn a_replayed_request_costs_what_the_first_one_did() {
    let service = service();

    for (target, _) in SHAPES {
        let first = counted(&service, target);
        let mut moved = Vec::new();

        for index in 0..10_000 {
            let counted = counted(&service, target);
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
