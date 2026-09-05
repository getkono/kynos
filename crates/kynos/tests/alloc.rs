//! What one request costs: what the routing path allocates, what a chain in
//! front of it adds, and how wide the future a driver holds is.
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
//!
//! The middleware half is recorded the same way and asks nothing of the same
//! kind: a chain is a slice run head-first, so what is worth pinning is that a
//! layer costs the same wherever it sits and that the future does not widen
//! with the stack. Both are measured over an interceptor that allocates
//! nothing, which is what leaves the excess as the chain's own machinery. The
//! width guard shares this fixture rather than joining
//! [`size.rs`](size.rs), which has no service to call.

#![cfg(feature = "macros")]

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use alloc_counter::count_alloc;
use kynos::{
    Router,
    extract::params::path::Path,
    http::Request,
    middleware::{Continued, Interceptor, Next},
    prelude::*,
    response::status::NoContent,
    router::service::Service,
};

/// The counter and the driver, shared so that a second counting target does
/// not copy them. Including this module is what installs the allocator.
#[path = "support/counting.rs"]
mod counting;

use counting::{counted, request};

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

/// The two operations, unmounted, so a stack can be put in front of them.
fn router() -> Router<()> {
    Router::<()>::new().mount(kynos::routes![ping, one])
}

fn service() -> Service<()> {
    router().build(()).expect("a describable router")
}

/// An interceptor that forwards and does nothing else, so that what a stack
/// costs is the chain's own machinery rather than the work a layer does.
///
/// Every associated type is the empty declaration —
/// [`Reads`](Interceptor::Reads) and [`Adds`](Interceptor::Adds) name no
/// header and [`Short`](Interceptor::Short) is
/// [`Infallible`](std::convert::Infallible) — which is also what lets eight
/// instances of *one* type mount: `CompatibleWith` compares `Adds::NAMES` and
/// `Short::STATUSES` for disjointness, and two empty sets are disjoint.
struct Transparent;

impl<C: Sync + 'static> Interceptor<C> for Transparent {
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Infallible> {
        let _ = (reads, context);
        Ok(next.run(request).await)
    }
}

/// Four layers. Written out rather than looped because `intercept` returns a
/// *different* `Router` type each time it is called, so a loop has no type to
/// iterate at; `build` is what erases the stack back to one `Service<()>`.
fn depth_4() -> Service<()> {
    router()
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .build(())
        .expect("a describable router")
}

/// Eight, for the same reason.
fn depth_8() -> Service<()> {
    router()
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .intercept(Transparent)
        .build(())
        .expect("a describable router")
}

/// One row of the table below: a depth, the service that mounts that many
/// layers, and what a request through it costs.
///
/// A named row rather than the tuple written inline, which Clippy reads as a
/// complex type — and it is one, since the builder cannot be a value: each
/// `intercept` call returns a different `Router` type, so the depths reach the
/// table as functions.
type Stack = (usize, fn() -> Service<()>, usize);

/// Every stack depth measured here, with what a request through it costs
/// today.
///
/// The target is the static match, so the excess over depth 0 is the stack's
/// alone: no capture is deserialized on the way.
const STACKS: [Stack; 3] = [
    // No stack at all: the same seven a static match costs in `SHAPES`.
    (0, service, 7),
    (4, depth_4, 11),
    (8, depth_8, 15),
];

/// The target every stack is measured against.
const STACKED: &str = "/ping";

/// What one layer adds, transcribed from the ceilings above: fifteen at depth
/// eight less seven at depth zero, over eight layers.
const PER_LAYER: usize = 1;

/// How wide the future a driver holds is allowed to be, measured rather than
/// chosen, and the same at every stack depth and under every feature
/// combination this target is built with.
const FUTURE_BYTES: usize = 280;

/// The record, for the middleware half: what one request costs at each depth,
/// over interceptors that allocate nothing of their own.
///
/// Seven allocations at depth 0, eleven at depth 4 and fifteen at depth 8 —
/// one heap allocation per layer, on top of the seven the routing path costs
/// with no stack in front of it. That one is the object-safe form of
/// `Interceptor` boxing the future it returns, which is the price of a
/// heterogeneous chain fitting in one slice.
///
/// Ceilings rather than targets, and measured rather than chosen, as
/// [`nfr.md`](../../../docs/nfr.md#thresholds) requires of a first
/// measurement. The relation these hold is
/// `a_layer_costs_the_same_wherever_it_sits`, which is what survives a change
/// to any of them.
#[test]
fn an_interceptor_stack_allocates_what_is_recorded_here() {
    for (depth, build, ceiling) in STACKS {
        let counted = counted(&build(), STACKED);
        assert!(
            counted <= ceiling,
            "a request through {depth} no-op interceptor(s) allocated \
             {counted} times against a recorded {ceiling}; raising a ceiling is \
             a change to docs/nfr.md, and lowering one is what making a layer \
             cheaper looks like"
        );
    }
}

/// The relation the stack ceilings are there to hold, and the one that survives
/// a change to any of them: a layer costs the same wherever it sits.
///
/// `Next::run` takes the head of a slice and awaits it rather than nesting one
/// chain inside another, so the eighth layer is no more expensive than the
/// first. Stated as `d8 + d0 == 2 * d4`, which is `d8 - d4 == d4 - d0` written
/// without a subtraction that could underflow before its message is read.
///
/// The control is the request that matched no route: dispatch answers it
/// before a chain exists to run, so eight layers cost it nothing. Without it a
/// count that grew with depth everywhere — the fixture leaking rather than the
/// chain costing — would read as the same result.
#[test]
fn a_layer_costs_the_same_wherever_it_sits() {
    let [(_, empty, _), (_, four, _), (deepest, eight, _)] = STACKS;
    let (d0, d4, d8) = (
        counted(&empty(), STACKED),
        counted(&four(), STACKED),
        counted(&eight(), STACKED),
    );

    assert!(
        d0 <= d4 && d4 <= d8,
        "a longer chain cost less than a shorter one (d0 = {d0}, d4 = {d4}, \
         d8 = {d8}); a saving that appears only as depth grows is a broken \
         measurement rather than a cheaper layer"
    );
    assert_eq!(
        d8 + d0,
        2 * d4,
        "the second four layers added {} allocation(s) where the first four \
         added {} (d0 = {d0}, d4 = {d4}, d8 = {d8}); a layer whose cost \
         depends on its depth means a chain nests rather than iterating a \
         slice",
        d8 - d4,
        d4 - d0
    );
    assert_eq!(
        d8 - d0,
        deepest * PER_LAYER,
        "{deepest} layers added {} allocation(s) against a recorded \
         {PER_LAYER} per layer; this is the number docs/nfr.md bills a layer \
         at",
        d8 - d0
    );

    let (missed_0, missed_8) = (counted(&empty(), "/nope"), counted(&eight(), "/nope"));
    assert_eq!(
        missed_0, missed_8,
        "a request matching no route cost {missed_0} with no stack and \
         {missed_8} behind eight layers; nothing that never reaches a chain \
         should notice how long one is"
    );
}

/// The other half of what a layer costs: the width of the future a driver
/// holds, and that a chain in front of it adds nothing to that width.
///
/// [`Service::call`] is an `async fn` over an erased dispatcher, so the stack
/// is gone from the type before any driver sees a future: all three depths
/// produce one future type, which is the only reason the array below compiles.
/// **That compile is the depth-invariance assertion.** The equality after it
/// cannot fail while the array stands, and is written out anyway because a
/// change that made the future carry its stack would have to delete the array
/// first — and a reader arriving at three separate `size_of_val` calls should
/// be able to see what was given up.
///
/// The ceiling is the half that can fail, and it is a ratchet rather than a
/// target: a future that widened would cost every in-flight request on the
/// server, which no allocation count above can see. 280 bytes, at every depth
/// and at both baseline and every feature. That is also the figure the request
/// for this guard named, but it is recorded here because it was measured — a
/// number carried over unmeasured would have pinned whatever it was guessed
/// at, and been indistinguishable from this one when it was wrong.
#[test]
fn a_driver_holds_one_future_whatever_the_chain_is() {
    let [(_, empty, _), (_, four, _), (_, eight, _)] = STACKS;
    let (at_0, at_4, at_8) = (empty(), four(), eight());

    // One array, so the three futures are one type or this does not build.
    let futures = [
        at_0.call(request(STACKED)),
        at_4.call(request(STACKED)),
        at_8.call(request(STACKED)),
    ];
    let [w0, w4, w8] = futures.map(|future| size_of_val(&future));

    assert_eq!(
        w4, w0,
        "four layers widened the dispatch future from {w0} to {w4} bytes"
    );
    assert_eq!(
        w8, w0,
        "eight layers widened the dispatch future from {w0} to {w8} bytes"
    );
    assert!(
        w0 <= FUTURE_BYTES,
        "the dispatch future is {w0} bytes against a recorded {FUTURE_BYTES}; \
         every request in flight carries one, so raising this ceiling is a \
         change to docs/nfr.md"
    );
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
/// Every shape and every stack depth is replayed, not only the parameterised
/// shape: a pair of tables that record six numbers and replay one would leave
/// five of them resting on a single reading. A chain is where the question is
/// sharpest — every layer holds an `Arc` and every call boxes a future, so a
/// clone that outlived its request would show here and nowhere else.
#[test]
fn a_replayed_request_costs_what_the_first_one_did() {
    let service = service();

    for (target, _) in SHAPES {
        replayed(&service, target, target);
    }

    for (depth, build, _) in STACKS {
        replayed(
            &build(),
            STACKED,
            &format!("{STACKED} behind {depth} no-op interceptor(s)"),
        );
    }
}

/// Ten thousand identical requests, against what the first one cost.
///
/// `described` names the case in the failure rather than being derived from
/// `target`, because the same target is replayed at three stack depths and a
/// message naming only the path would not say which one moved. Both are built
/// outside every counted region, so neither costs the measurement anything.
fn replayed(service: &Service<()>, target: &str, described: &str) {
    let first = counted(service, target);
    let mut moved = Vec::new();

    for index in 0..10_000 {
        let counted = counted(service, target);
        if counted != first {
            moved.push((index, counted));
        }
    }

    assert!(
        moved.is_empty(),
        "{described} allocated {first} times on one request and differently \
         on {} of the next ten thousand, starting at {:?}; a count that moves \
         between identical requests is state accumulating on the routing path",
        moved.len(),
        moved.first()
    );
}
