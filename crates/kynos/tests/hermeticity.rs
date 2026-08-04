//! Guards the test runner contract: every test gets a fresh process.
//!
//! Both tests observe the same `static`, so they can only both see its initial
//! value when each runs in its own process. They pass under `cargo nextest run`
//! and fail under `cargo test`, which shares one process across a test binary.

use std::sync::atomic::{AtomicUsize, Ordering};

static OBSERVATIONS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn process_state_is_not_shared_with_the_sibling_test() {
    assert_eq!(OBSERVATIONS.fetch_add(1, Ordering::Relaxed), 0);
}

#[test]
fn process_state_is_not_shared_with_the_preceding_test() {
    assert_eq!(OBSERVATIONS.fetch_add(1, Ordering::Relaxed), 0);
}
