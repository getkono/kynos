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
