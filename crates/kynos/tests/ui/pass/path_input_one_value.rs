//! One path value is extracted from the path, the control for
//! `traits/path_input.rs`'s type that is neither a group nor a value.

fn extracted<T: kynos::extract::params::path::PathInput>() {}

fn main() {
    extracted::<u64>();
}
