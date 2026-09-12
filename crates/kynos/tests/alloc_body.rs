//! What erasing a body through the boxed trait object costs, counted.
//!
//! The allocation-count kind in
//! [`performance.md`](../../../docs/performance.md#the-taxonomy), and the
//! second target carrying a `#[global_allocator]`. A second one is what the
//! first one's reason asks for: an allocator is process-wide, so a target that
//! installs one measures its own binary and nothing else, and folding these
//! counts into [`alloc.rs`](alloc.rs) would put a body constructor's cost into
//! the file whose numbers are the routing path's.
//!
//! **The entry this answers predicted the wrong thing.**
//! [`architecture.md`](../../../docs/architecture.md#why-hyper-stays) names
//! erasing every body as a cheap win worth "once per request". It is not one of
//! the seven [`alloc.rs`](alloc.rs) records: the request body there is built
//! before the counted region opens, and the response body a `204` sends is
//! `Body::empty`, which erases a zero-sized type — and `Box::pin` of a
//! zero-sized value allocates nothing. What the entry describes is real one
//! step further out, on the server path, where `Body::from_incoming` erases a
//! `hyper::body::Incoming` that is not zero-sized. Three `size_of` witnesses
//! hold those reasons, and they live in [`size.rs`](size.rs) where
//! [`performance.md`](../../../docs/performance.md#the-taxonomy) files a size
//! guard: nothing about a `size_of` needs the allocator this target installs,
//! and a dependency bump that ends any of the three turns something red there.
//!
//! **Both rows are held exactly, and neither of them is a ceiling.**
//! [`alloc.rs`](alloc.rs)'s numbers are ceilings because they record a
//! requirement that is not met and are meant to fall. These two are not going
//! anywhere: zero is the floor and the reason for it is a zero-sized type, and
//! the one is the whole of what erasing a body that is not empty does. A count
//! that came in *under* either row would mean the boxing had stopped happening,
//! which contradicts the verdict exactly as much as a count over it does, so
//! both are asserted with `==`.
//!
//! Ungated. Nothing here names a derive, a router or a runtime — only
//! `kynos::http`, which is behind no feature — so the numbers hold at every
//! feature set `features:targets` builds.

use alloc_counter::{AllocCounterSystem, count_alloc};
use bytes::Bytes;
use kynos::http::body::Body;

/// Declared here rather than reached for: `alloc_counter` installs nothing on
/// its own behalf, so this line is the whole of what puts the counter in this
/// binary and in no other.
#[global_allocator]
static ALLOCATOR: AllocCounterSystem = AllocCounterSystem;

/// Every constructor measured here, with what it costs today.
///
/// Each number is the measurement rather than a target, as
/// [`nfr.md`](../../../docs/nfr.md#thresholds) requires of a first measurement
/// — and each is held as an equality, because neither is a threshold with room
/// underneath it. Erasing an empty body must stay free, and erasing a body that
/// is not empty must stay a single allocation: the second is the structural
/// claim [`architecture.md`](../../../docs/architecture.md#why-hyper-stays)
/// rests its verdict on, so a count of zero there would falsify the verdict
/// rather than beat it.
const RECORDED: [(&str, usize); 2] = [("Body::empty()", 0), ("Body::from_bytes(..)", 1)];

/// Constructs one body and reports the heap operations the construction made.
///
/// Fresh allocations and reallocations both, so that growing a buffer cannot
/// pass as free. Whatever the constructor is handed is built before the region
/// opens and the body is dropped after it closes, because a caller's bytes and
/// a body's teardown are not what erasure costs.
fn counted(construct: impl FnOnce() -> Body) -> usize {
    let ((allocations, reallocations, _), body) = count_alloc(construct);
    drop(body);
    allocations + reallocations
}

/// The record.
#[test]
fn erasing_a_body_costs_what_the_table_records() {
    // Static bytes: `Bytes::from_static` owns no allocation to begin with, so
    // the second row measures the erasure and not the payload behind it.
    let payload = Bytes::from_static(b"{\"ok\":true}");

    // Typed to the record's own length, so a row added above without a
    // constructor added here is a compile error rather than a row `zip`
    // silently drops.
    let measured: [usize; RECORDED.len()] = [
        counted(Body::empty),
        counted(move || Body::from_bytes(payload)),
    ];

    for ((constructor, recorded), counted) in RECORDED.into_iter().zip(measured) {
        assert_eq!(
            counted, recorded,
            "{constructor} allocated {counted} time(s) against a recorded \
             {recorded}; a move in either direction is a change to the verdict \
             in docs/architecture.md, which says what erasing a body costs"
        );
    }
}

/// A constructor that allocates a known amount before erasing an empty body, so
/// that part of a reading taken through it is fixed by construction rather than
/// measured.
///
/// Two heap operations, deliberately one of each kind. `Vec::with_capacity` is
/// one fresh allocation; extending past that capacity is one *reallocation*,
/// because a `Vec` that outgrows its buffer asks the allocator to resize it
/// rather than to hand out a second one. A driver that reported only the first
/// would be counting half of what [`counted`] says it counts.
///
/// [`black_box`](std::hint::black_box) is what keeps both from being optimized
/// away: nothing reads the buffer, and a dead `Vec` is exactly the shape a
/// compiler is free to delete.
///
/// It ends in `Body::empty()` because that is the control it is read against —
/// the same constructor, differing only in the two operations performed in
/// front of it.
fn calibrating() -> Body {
    let mut buffer = Vec::<u8>::with_capacity(1);
    buffer.extend_from_slice(&[0, 0]);
    drop(std::hint::black_box(buffer));

    Body::empty()
}

/// What [`calibrating`] adds to a construction, by construction: one fresh
/// allocation and one reallocation.
///
/// A constructed target rather than a recorded measurement: it is what
/// [`calibrating`]'s body does, not what a run reported. That is the ground for
/// holding it at an equality, and it is the ground both rows of [`RECORDED`]
/// are already held at equalities on — a count under either of those would mean
/// the boxing had stopped happening, and a count under this one would mean the
/// counting had. It is also why nothing re-reads it when a recorded row moves:
/// none of what it counts is a body constructor's.
///
/// Confirmed against the instrument all the same, the way both recorded rows
/// were: set to zero, and the delta transcribed out of the failure. Two at
/// baseline and two with `--all-features`, which is every shape this ungated
/// target is built at.
const CALIBRATION: usize = 2;

/// The instrument's invariant, and the one both numbers above rest on: a count
/// is *every* heap operation the region saw, fresh allocations and
/// reallocations alike.
///
/// **Neither recorded row could see this, and the reason is not the one
/// [`alloc.rs`](alloc.rs) gives.** There the numbers are ceilings compared with
/// `<=`, so a count that falls passes silently. Both rows here are equalities,
/// so a fall on the second — `Body::from_bytes` reading 0 against a recorded 1
/// — is red already. What no assertion here could see is the *reallocation*
/// half of the sum: between them the two constructors perform one fresh
/// allocation and grow no buffer, so a driver summing `allocations` alone
/// reports exactly what this file records. Measured, before this fixture
/// existed: dropping `reallocations` from [`counted`] left
/// `cargo nextest run -p kynos --test alloc_body` at 1 passed. The half of a
/// sum that a target's own numbers never exercise is the half that needs a
/// fixture performing it on purpose.
///
/// **Why a delta rather than the absolute, when the absolute is available.**
/// `Body::empty()` allocates nothing and is recorded above as allocating
/// nothing, so the two forms agree today and `assert_eq!(calibrated, 2)` would
/// hold as written. That is not true in [`alloc.rs`](alloc.rs), where the
/// region carries a routing path's irreducible cost and an absolute would be
/// mostly the router's. The delta is kept here for the other half of that
/// reason: read against the control, this assertion says what [`calibrating`]
/// *adds*, which is a property of the counter; written as an absolute it would
/// also be asserting that erasing an empty body is free, which is the first
/// recorded row's claim and the first recorded row's to fail. One red test per
/// defect, naming the thing that broke.
///
/// **What this cannot catch, since only a docblock can hold it.** A delta
/// cancels any shift the two readings share, so a driver that under-reported
/// every construction by the same amount would be invisible here. The recorded
/// rows bound that case without closing it: a uniform under-report cannot take
/// `Body::empty`'s zero any lower, so it survives only by leaving the floor
/// alone, and the equality on the second row is what catches a uniform shift of
/// one. A driver mis-reporting only regions costlier than either constructor
/// ever is remains unseen. The blind spot is accepted and written down rather
/// than closed, as it is in the assertion this one follows.
///
/// **Filed here rather than folded onto the shared harness.** #133's recorded
/// decision refused that fold, the operative ground being that this target
/// measures a body constructor through `impl FnOnce() -> Body` with no
/// `Service`, no `Request` and no poll — so it shares no driver with
/// [`alloc.rs`](alloc.rs), and the assertion filed there for the two targets
/// that *do* share one reaches nothing in this binary. A second driver is a
/// second thing to hold.
#[test]
fn the_counter_reports_every_heap_operation_in_the_region() {
    let (empty, calibrated) = (counted(Body::empty), counted(calibrating));

    // Stated as an addition rather than as `calibrated - empty`, so a reading
    // that fell below its control cannot underflow before its message is read
    // — the form `the_counter_reports_every_heap_operation_in_the_region` in
    // `alloc.rs` uses, for the same reason.
    assert_eq!(
        calibrated,
        empty + CALIBRATION,
        "erasing an empty body behind two deliberate heap operations cost \
         {calibrated} against the {empty} the same constructor costs alone, a \
         difference of {} where {CALIBRATION} were performed by construction — \
         one fresh allocation and one reallocation. A driver that stopped \
         counting one of the two kinds reports fewer here, and both rows \
         recorded above would pass it",
        calibrated.saturating_sub(empty)
    );
}
