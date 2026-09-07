//! Size guards, the kind [`performance.md`](../../../docs/performance.md#the-taxonomy)
//! files here.
//!
//! Two subjects, and they are guards in opposite directions.
//! [`Error`](kynos::Error) is `#[non_exhaustive]`, so a variant can be added
//! without a caller noticing — including one wide enough to widen every
//! `kynos::Result` in the crate. Those numbers were measured rather than
//! chosen, as [`nfr.md`](../../../docs/nfr.md#thresholds) requires, and they
//! are ceilings rather than targets: the relations are what matter, and the
//! absolute bounds are deliberately loose.
//!
//! The three body witnesses are floors instead, and they are the reasons
//! [`alloc_body.rs`](alloc_body.rs)'s counts read the way they do. Boxing a
//! zero-sized value allocates nothing, so whether each erased body is
//! zero-sized is what decides its row; a dependency bump that moves one turns
//! the fact red here rather than leaving only the number red there. They need
//! no allocator, which is why they are not in that target.
//!
//! Ungated. Nothing here names a derive, a router or a runtime.

use std::{convert::Infallible, error::Error as StdError, mem::size_of_val};

use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full};
use kynos::{Error, openapi::Violation};

/// What the erased error type is, spelled the way `body.rs` spells it, so the
/// witnesses below measure the type the library actually erases rather than a
/// cheaper relative of it.
type BoxError = Box<dyn StdError + Send + Sync>;

/// `Error::Invalid` is the variant all four `Router` methods return, and it
/// carries every violation found rather than the first. Holding them behind a
/// `Vec` is what keeps that free: a single `Violation` is wider than the whole
/// error, so inlining even one would cost more than the list does.
#[test]
fn a_build_failure_does_not_inline_a_violation() {
    let error = size_of::<Error>();
    let violation = size_of::<Violation>();

    assert!(
        error < violation,
        "Error ({error} bytes) should not inline Violation ({violation} bytes); \
         the violation list must stay behind a Vec"
    );
    assert!(
        error <= 64,
        "Error grew to {error} bytes; box the payload of any wide variant that was added"
    );
}

/// Six of the seven functions returning a `kynos::Result` succeed on every run
/// that is not a misconfiguration, so the failure path should cost the success
/// path nothing. `Error` has far fewer variants than its tag can express, and
/// the discriminant lands in that niche.
#[test]
fn a_build_result_costs_no_more_than_its_failure() {
    let result = size_of::<kynos::Result<()>>();
    let error = size_of::<Error>();

    assert_eq!(
        result, error,
        "Result<(), Error> ({result} bytes) should be no wider than Error ({error} bytes); \
         the Ok/Err discriminant must fit Error's own tag niche"
    );
}

/// Why `alloc_body.rs`'s first row is zero, and the property the whole verdict
/// in [`architecture.md`](../../../docs/architecture.md#why-hyper-stays) rests
/// on.
///
/// `Body::empty` erases `Empty<Bytes>` mapped into the boxed error, and
/// `boxed_unsync` is a `Box::pin` — which for a zero-sized value returns a
/// dangling pointer and touches the allocator not at all. The type is rebuilt
/// here rather than named through the library, because the erased type is
/// private by design and naming it publicly is what these witnesses exist to
/// avoid asking for. A dependency bump that gives `Empty` a field turns that
/// first row red as well — what this adds is *which* fact broke, and it is the
/// fact `architecture.md`'s verdict is written on rather than the number.
#[test]
fn an_empty_body_erases_a_zero_sized_type() {
    let erased = Empty::<Bytes>::new().map_err(|never: Infallible| -> BoxError { match never {} });

    let size = size_of_val(&erased);
    assert_eq!(
        size, 0,
        "the body `Body::empty` erases is {size} bytes rather than zero, so \
         boxing it now allocates; the zero recorded in tests/alloc_body.rs is \
         no longer free and docs/architecture.md's verdict on it is stale"
    );
}

/// Why `alloc_body.rs`'s second row is one: the same erasure of a body that
/// carries bytes has something to put on the heap.
#[test]
fn a_body_holding_bytes_is_not_zero_sized() {
    let erased = Full::new(Bytes::from_static(b"{\"ok\":true}"))
        .map_err(|never: Infallible| -> BoxError { match never {} });

    assert!(
        size_of_val(&erased) > 0,
        "a body carrying bytes is zero-sized, which would make the allocation \
         recorded for `Body::from_bytes` something other than the boxing"
    );
}

/// Where the entry's "once per request" is actually true.
///
/// `Body::from_incoming` is the server path's erasure, and what it erases is
/// hyper's own body. It is not zero-sized, so every request that arrives over a
/// socket costs the `Box::pin` `alloc_body.rs`'s second row measures — outside
/// the region [`alloc.rs`](alloc.rs) counts, which is why none of its seven is
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
