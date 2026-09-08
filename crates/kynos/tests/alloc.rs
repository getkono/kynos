//! What one request costs: what the routing path allocates, what a chain in
//! front of it adds, and how wide the future dispatch returns is.
//!
//! The allocation-count kind in
//! [`performance.md`](../../../docs/performance.md#the-taxonomy), and one of
//! the four targets that document says own the global allocator. It is a target
//! of its own rather than a sibling `tests.rs` beside the router because a
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
    http::{Method, Request, StatusCode},
    middleware::{Continued, Interceptor, Next},
    prelude::*,
    response::status::NoContent,
    router::service::Service,
};

/// The counter, the request builder and the driver, shared with
/// `alloc_codecs.rs` so that the second counting target does not carry a copy
/// of them. Including this module is what installs the allocator.
#[path = "support/counting.rs"]
mod counting;

use counting::request;

/// Every shape measured here, with what it costs today.
///
/// `/users/{id}` is dispatch *and* the `Path` extractor that reads the capture,
/// so its excess over `/ping` is not the router's alone. Splitting the two is
/// the attribution [`nfr.md`](../../../docs/nfr.md#routing) names as the next
/// piece of work.
///
/// The status is the one the count has to be of, for the reason
/// [`alloc_codecs.rs`](alloc_codecs.rs) gives: a request answered 404 where 204
/// was meant is a count of a miss rather than of a match, and every ceiling
/// here is a `<=` that such a count would pass under.
const SHAPES: [(&str, StatusCode, usize); 3] = [
    // A static match, with no parameter to capture. Also the row `STACKED`
    // and the depth-0 stack ceiling are read from.
    ("/ping", StatusCode::NO_CONTENT, 7),
    // One path parameter, captured and deserialized.
    ("/users/7", StatusCode::NO_CONTENT, 11),
    // A request matching no route at all.
    ("/nope", StatusCode::NOT_FOUND, 6),
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

/// One `GET` against `target`, driven through the shared driver and dropped.
///
/// The shape every reading in this file is taken in. The region it is taken
/// over is [`counting::counted`]'s, shared with `alloc_codecs.rs` so that a
/// correction to one target's driver cannot leave the other behind.
fn counted(service: &Service<()>, target: &str, expected: StatusCode) -> usize {
    let (allocations, response) =
        counting::counted(service, request(Method::GET, target, None, b""), expected);

    drop(response);
    allocations
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

/// One transparent layer: the control the calibration below is read against.
///
/// The same depth as [`calibrated`] and the same builder, differing only in
/// which interceptor is mounted, so that the difference between two readings
/// taken through them is what [`Calibrating`] does and nothing else.
fn depth_1() -> Service<()> {
    router()
        .intercept(Transparent)
        .build(())
        .expect("a describable router")
}

/// An interceptor that allocates a known amount, so that part of a count taken
/// through it is fixed by construction rather than measured.
///
/// Two heap operations, deliberately one of each kind. `Vec::with_capacity` is
/// one fresh allocation; extending past that capacity is one *reallocation*,
/// because a `Vec` that outgrows its buffer asks the allocator to resize it
/// rather than to hand out a second one. A counter that reported only the
/// first would be counting half of what every ceiling in this target and in
/// `alloc_codecs.rs` is recorded in.
///
/// [`black_box`](std::hint::black_box) is what keeps both from being optimized
/// away: nothing reads the buffer, and a dead `Vec` is exactly the shape a
/// compiler is free to delete.
struct Calibrating;

impl<C: Sync + 'static> Interceptor<C> for Calibrating {
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

        let mut buffer = Vec::<u8>::with_capacity(1);
        buffer.extend_from_slice(&[0, 0]);
        drop(std::hint::black_box(buffer));

        Ok(next.run(request).await)
    }
}

/// The routing fixture with one calibrating layer in front of it.
fn calibrated() -> Service<()> {
    router()
        .intercept(Calibrating)
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

/// The target every stack is measured against: the static match, so the excess
/// over depth 0 is the stack's alone, with no capture deserialized on the way.
///
/// Read out of [`SHAPES`] rather than written again, so that the two tables
/// cannot disagree about which shape is stacked.
const STACKED: &str = SHAPES[0].0;

/// The answer that target has to give for a count to be of the match, from the
/// same row, for the reason [`SHAPES`] gives.
const STACKED_STATUS: StatusCode = SHAPES[0].1;

/// What that target costs with no stack in front of it, from the same row: the
/// depth-0 ceiling below *is* the static match's, so re-measuring one moves
/// both.
const STACKED_ALONE: usize = SHAPES[0].2;

/// The control every stack is read against: the request that matched no route,
/// read out of [`SHAPES`] for the reason [`STACKED`] is.
const MISSED: &str = SHAPES[2].0;

/// The answer *that* target has to give, from the same row.
const MISSED_STATUS: StatusCode = SHAPES[2].1;

/// The shape whose excess over the static match is what a capture costs, read
/// out of [`SHAPES`] for the reason [`STACKED`] is.
const CAPTURED: &str = SHAPES[1].0;

/// The answer *that* target has to give, from the same row.
const CAPTURED_STATUS: StatusCode = SHAPES[1].1;

/// Every stack depth measured here, with what a request through it costs
/// today.
///
/// Depth 0 is the baseline the other two are read against rather than a row
/// this file asserts on its own: its builder, target and ceiling are the
/// static match's, which
/// `the_routing_path_allocates_where_the_requirement_asks_for_nothing` already
/// holds. Only the stacked rows are counted and replayed below.
const STACKS: [Stack; 3] = [
    // No stack at all: what the same target costs in `SHAPES`, not a second
    // recording of it.
    (0, service, STACKED_ALONE),
    (4, depth_4, 11),
    (8, depth_8, 15),
];

/// What one layer adds, transcribed from the ceilings above: fifteen at depth
/// eight less seven at depth zero, over eight layers.
const PER_LAYER: usize = 1;

/// How wide the future [`Service::call`] returns is allowed to be, measured
/// rather than chosen, and the same at every stack depth.
///
/// The *dispatch* future alone, which is a lower bound on what a driver holds
/// rather than the cost of one request in flight: the driver at
/// [`server/connection.rs`](../src/server/connection.rs) hands hyper an
/// enclosing `async` block that carries this future along with the request it
/// rebuilt and the handle it called through, and hyper holds that inside
/// per-connection state of its own. Neither is measured here.
///
/// Read at both feature sets this target is built at, by setting the ceiling
/// to zero and taking the width out of the failure: 280 bytes at baseline
/// (`cargo nextest run -p kynos --test alloc`) and 280 with `--all-features`.
/// Only `<=` is asserted per build, so this is where the two readings are
/// recorded — an equality would fail on the first build whose feature set
/// makes the future narrower, which is not a regression.
const DISPATCH_FUTURE_BYTES: usize = 280;

/// The record, for the middleware half: what one request costs at each depth a
/// stack is mounted at, over interceptors that allocate nothing of their own.
///
/// Eleven allocations at depth 4 and fifteen at depth 8, against the
/// [`STACKED_ALONE`] seven the routing path costs with no stack in front of
/// it — one heap allocation per layer. That one is the object-safe form of
/// `Interceptor` boxing the future it returns, which is the price of a
/// heterogeneous chain fitting in one slice.
///
/// Depth 0 is skipped rather than measured again here: it is the same builder,
/// the same target and the same ceiling
/// `the_routing_path_allocates_where_the_requirement_asks_for_nothing`
/// already asserts. It stays in the table as the baseline the delta is taken
/// against.
///
/// Ceilings rather than targets, and measured rather than chosen, as
/// [`nfr.md`](../../../docs/nfr.md#thresholds) requires of a first
/// measurement. The relation these hold is
/// `a_layer_costs_the_same_wherever_it_sits`, which is what survives a change
/// to any of them.
#[test]
fn an_interceptor_stack_allocates_what_is_recorded_here() {
    for &(depth, build, ceiling) in &STACKS[1..] {
        let counted = counted(&build(), STACKED, STACKED_STATUS);
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
        counted(&empty(), STACKED, STACKED_STATUS),
        counted(&four(), STACKED, STACKED_STATUS),
        counted(&eight(), STACKED, STACKED_STATUS),
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

    let (missed_0, missed_8) = (
        counted(&empty(), MISSED, MISSED_STATUS),
        counted(&eight(), MISSED, MISSED_STATUS),
    );
    assert_eq!(
        missed_0, missed_8,
        "a request matching no route cost {missed_0} with no stack and \
         {missed_8} behind eight layers; nothing that never reaches a chain \
         should notice how long one is"
    );
}

/// The other half of what a layer costs: the width of the future
/// [`Service::call`] returns, and that a chain in front of it adds nothing to
/// that width.
///
/// [`Service::call`] is an `async fn` over an erased dispatcher, so the stack
/// is gone from the type before any caller sees a future: all three depths
/// produce one future type, which is the only reason the array below compiles.
/// **That compile is the whole of the depth-invariance assertion**, so nothing
/// below re-states it as an equality that could not fail.
///
/// The ceiling is the half that can fail, and it is a ratchet rather than a
/// target: a future that widened would cost every in-flight request on the
/// server, which no allocation count above can see. 280 bytes at every depth,
/// and 280 at each of the two feature sets this target is built at — see
/// [`DISPATCH_FUTURE_BYTES`] for the readings and for what the number does not
/// cover. That is also the figure the request for this guard named, but it is
/// recorded here because it was measured — a number carried over unmeasured
/// would have pinned whatever it was guessed at, and been indistinguishable
/// from this one when it was wrong.
#[test]
fn a_chain_does_not_widen_the_dispatch_future() {
    let [(_, empty, _), (_, four, _), (_, eight, _)] = STACKS;
    let (at_0, at_4, at_8) = (empty(), four(), eight());

    // One array, so the three futures are one type or this does not build.
    // That compile *is* the depth-invariance assertion: a change that made the
    // future carry its stack would have to delete this array first. Three
    // readings of one type cannot differ, so none is asserted against another.
    let futures = [
        at_0.call(request(Method::GET, STACKED, None, b"")),
        at_4.call(request(Method::GET, STACKED, None, b"")),
        at_8.call(request(Method::GET, STACKED, None, b"")),
    ];
    let [width, ..] = futures.map(|future| size_of_val(&future));

    assert!(
        width <= DISPATCH_FUTURE_BYTES,
        "the dispatch future is {width} bytes against a recorded \
         {DISPATCH_FUTURE_BYTES}; every request in flight carries one, so \
         raising this ceiling is a change to docs/nfr.md"
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

/// What [`Calibrating`] adds to a request, by construction: one fresh
/// allocation and one reallocation.
///
/// A constructed target rather than a recorded measurement: it is what
/// [`Calibrating`]'s body does, not what a run reported. That is the ground
/// for holding it at an equality, and it is the ground
/// [`nfr.md`](../../../docs/nfr.md#routing) already gives for holding body
/// erasure at one — "because a count under either would mean the boxing had
/// stopped happening". A count under this one would mean the counting had.
/// It is also why nothing re-reads it when a ceiling moves: none of what it
/// counts is the router's.
///
/// Confirmed against the instrument all the same, the way every recorded
/// number here was read: set to zero, and the delta transcribed out of the
/// failure. Two at baseline (`cargo nextest run -p kynos --test alloc`) and
/// two with `--all-features`, the two configurations this target is built at.
const CALIBRATION: usize = 2;

/// The instrument's second invariant, and the one every ceiling in either
/// counting target rests on: a count is *every* heap operation the region saw,
/// fresh allocations and reallocations alike.
///
/// **Stated as a delta rather than as an absolute, because an absolute would
/// be mostly the router's.** One request through [`calibrated`] costs ten
/// today, of which eight is the routing path's [`STACKED_ALONE`] and the boxed
/// future one layer costs — both recorded above as ceilings, and both free to
/// fall. Pinning the ten would turn a rustc or dependency bump that made the
/// static match one allocation cheaper into a red *instrument* test: every
/// ceiling would pass, both equalities over differences would pass, and this
/// would be the only failure in either target, saying the driver had changed
/// when the router had merely got cheaper. Reading it against a transparent
/// layer at the same depth cancels all eight. What is left is what
/// [`Calibrating`] does, which nothing outside this file can move — the
/// arrangement [`performance.md`](../../../docs/performance.md#thresholds)
/// asks for, where relations outlive absolutes.
///
/// **Why no ceiling could see this.** Dropping `reallocations` from the
/// driver's sum was measured to move nine `alloc_codecs.rs` compression
/// readings down — gzip 27→26, 27→26, 31→27; br 42→41, 42→41, 48→45; zstd
/// 21→20, 21→20, 24→21 — so reallocations are counted there, and often. What
/// no *assertion* could see is that they fell: the ceilings are `<=`, the leak
/// replays read a uniform fall as still constant, and the strict relations and
/// the equalities over differences are all one-sided.
///
/// **What this cannot catch, since only a docblock can hold it.** The region
/// is "construct the future, then poll it", and an `async fn`'s construction
/// allocates nothing, so a narrowing of the region's *front* boundary moves no
/// count — measured, by moving `Service::call` outside the region and watching
/// both targets stay green. No fixture can reach that boundary either: the
/// only handle one has on the inside is `Interceptor::intercept`, which is
/// itself an `async fn`. It becomes a real hole the day dispatch boxes at call
/// time rather than at poll time.
///
/// Filed here beside `work_on_another_thread_is_not_counted` rather than in
/// `alloc_codecs.rs`, for that assertion's reason and by the precedent
/// [`testing.md`](../../../docs/testing.md#hermeticity) sets: the property
/// belongs to `alloc_counter` and to the shared driver rather than to any
/// fixture, so it is asserted once for both targets. Since #133 there is one
/// driver, which is what lets one assertion reach both — and is why it has to
/// exist, because a single edit to that driver now moves every recorded number
/// in both files at once and leaves the two as comparable as they ever were.
#[test]
fn the_counter_reports_every_heap_operation_in_the_region() {
    let (plain, calibrating) = (
        counted(&depth_1(), STACKED, STACKED_STATUS),
        counted(&calibrated(), STACKED, STACKED_STATUS),
    );

    // Stated as an addition rather than as `calibrating - plain`, so a reading
    // that fell below its control cannot underflow before its message is read
    // — the form `a_layer_costs_the_same_wherever_it_sits` uses, for the same
    // reason.
    assert_eq!(
        calibrating,
        plain + CALIBRATION,
        "one calibrating layer added {} heap operation(s) to a request that \
         cost {plain} through a transparent one, against the {CALIBRATION} it \
         performs by construction — one fresh allocation and one \
         reallocation. A driver that stopped counting either kind reports \
         fewer here, and every `<=` ceiling in this target and in \
         alloc_codecs.rs would pass it",
        calibrating.saturating_sub(plain)
    );
}

/// The record. Named so it reads as one: each ceiling is what the path costs
/// today, and none of them is zero.
#[test]
fn the_routing_path_allocates_where_the_requirement_asks_for_nothing() {
    let service = service();

    for (target, expected, ceiling) in SHAPES {
        let counted = counted(&service, target, expected);
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

    // Each shape reaches this test through the const that names it, rather
    // than by destructuring `SHAPES` here. That centralises the row-position
    // binding rather than removing it — `STACKED`, `CAPTURED` and `MISSED` are
    // still `SHAPES[0].0`, `[1].0` and `[2].0` — but it puts the binding in one
    // place, beside the doc that says which shape each names, instead of
    // three hundred lines away in a destructure that would silently rebind
    // `matched`, `captured` and `missed` and leave this test passing about the
    // wrong three requests. Removing it outright wants named fields, which is
    // what `alloc_codecs.rs`'s `Table` is a struct rather than a
    // `[Measured; 5]` for.
    let matched = counted(&service, STACKED, STACKED_STATUS);
    let captured = counted(&service, CAPTURED, CAPTURED_STATUS);
    let missed = counted(&service, MISSED, MISSED_STATUS);

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
/// Every shape and every *stacked* depth is replayed, not only the
/// parameterised shape: a pair of tables that record six numbers and replay one
/// would leave five of them resting on a single reading. Depth 0 is the
/// [`STACKED`] row of [`SHAPES`], replayed against the same service by the loop
/// above. A chain is where the question is sharpest — every layer holds an
/// `Arc` and every call boxes a future, so a clone that outlived its request
/// would show here and nowhere else.
#[test]
fn a_replayed_request_costs_what_the_first_one_did() {
    let service = service();

    for (target, expected, _) in SHAPES {
        replayed(&service, target, expected, target);
    }

    for &(depth, build, _) in &STACKS[1..] {
        replayed(
            &build(),
            STACKED,
            STACKED_STATUS,
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
fn replayed(service: &Service<()>, target: &str, expected: StatusCode, described: &str) {
    let first = counted(service, target, expected);
    let mut moved = Vec::new();

    for index in 0..10_000 {
        let counted = counted(service, target, expected);
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
