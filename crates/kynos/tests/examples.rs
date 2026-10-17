//! Examples run to completion.
//!
//! An example's assertions only hold if something runs them, and no task runs
//! `cargo run --example`. Each example here is included as a module and its
//! `main` driven directly, so a declared response it stops exercising fails
//! this target rather than the first reader to copy it.

#![cfg(all(
    feature = "openapi31",
    feature = "macros",
    feature = "json",
    feature = "test-util"
))]

#[path = "../examples/testing.rs"]
mod testing;

/// `examples/testing.rs` asserts both conformance directions, so it returns
/// only if every response it declares was exercised and every response it saw
/// was declared.
#[test]
fn testing_example_runs_to_completion() {
    testing::main().expect("the example's service builds");
}
