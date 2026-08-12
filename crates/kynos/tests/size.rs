//! Size guards for the framework failure.
//!
//! [`Error`](kynos::Error) is `#[non_exhaustive]`, so a variant can be added
//! without a caller noticing — including one wide enough to widen every
//! `kynos::Result` in the crate. These assertions are what makes that visible.
//!
//! The numbers were measured rather than chosen, as
//! [`nfr.md`](../../../docs/nfr.md#thresholds) requires, and they are ceilings
//! rather than targets: the relations are what matter, and the absolute bounds
//! are deliberately loose.

use kynos::{Error, openapi::Violation};

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
