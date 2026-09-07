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
//! hold those reasons: two say why the table below reads the way it does, and
//! the third pins the type the server path erases, so a dependency bump that
//! ends any of the three turns something red.
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

use std::{convert::Infallible, error::Error as StdError, mem::size_of_val};

use alloc_counter::{AllocCounterSystem, count_alloc};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use kynos::http::body::Body;

/// Declared here rather than reached for: `alloc_counter` installs nothing on
/// its own behalf, so this line is the whole of what puts the counter in this
/// binary and in no other.
#[global_allocator]
static ALLOCATOR: AllocCounterSystem = AllocCounterSystem;

/// What the erased error type is, spelled the way `body.rs` spells it, so the
/// witness below measures the type the library actually erases rather than a
/// cheaper relative of it.
type BoxError = Box<dyn StdError + Send + Sync>;

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

/// Why the first row is zero, and the property the whole verdict rests on.
///
/// `Body::empty` erases `Empty<Bytes>` mapped into the boxed error, and
/// `boxed_unsync` is a `Box::pin` — which for a zero-sized value returns a
/// dangling pointer and touches the allocator not at all. The type is rebuilt
/// here rather than named through the library, because the erased type is
/// private by design and naming it publicly is what this file exists to avoid
/// asking for. A dependency bump that gives `Empty` a field turns the first row
/// above red as well — what this adds is *which* fact broke, and it is the
/// fact `architecture.md`'s verdict is written on rather than the number.
#[test]
fn an_empty_body_erases_a_zero_sized_type() {
    let erased = Empty::<Bytes>::new().map_err(|never: Infallible| -> BoxError { match never {} });

    let size = size_of_val(&erased);
    assert_eq!(
        size, 0,
        "the body `Body::empty` erases is {size} bytes rather than zero, so \
         boxing it now allocates; the zero in the table above is no longer \
         free and docs/architecture.md's verdict on it is stale"
    );
}

/// Why the second row is one: the same erasure of a body that carries bytes has
/// something to put on the heap.
#[test]
fn a_body_holding_bytes_is_not_zero_sized() {
    let erased = Full::new(Bytes::from_static(b"{\"ok\":true}"))
        .map_err(|never: Infallible| -> BoxError { match never {} });

    assert!(
        size_of_val(&erased) > 0,
        "a body carrying bytes is zero-sized, which would make the recorded \
         allocation for `Body::from_bytes` something other than the boxing"
    );
}

/// Where the entry's "once per request" is actually true.
///
/// `Body::from_incoming` is the server path's erasure, and what it erases is
/// hyper's own body. It is not zero-sized, so every request that arrives over a
/// socket costs the `Box::pin` the second row above measures — outside the
/// region [`alloc.rs`](alloc.rs) counts, which is why none of its seven is
/// this one.
#[test]
fn the_body_the_server_erases_is_not_zero_sized() {
    let size = size_of::<hyper::body::Incoming>();
    assert!(
        size > 0,
        "hyper's incoming body is zero-sized, so erasing it would be free and \
         the server path would cost one allocation fewer per request than \
         docs/architecture.md records"
    );
}
