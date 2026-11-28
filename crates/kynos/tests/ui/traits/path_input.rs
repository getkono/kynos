//! A type that is neither a path parameter group nor one path value is not
//! extracted from the path.

struct NotAPathInput;

fn extracted<T: kynos::extract::params::path::PathInput>() {}

fn main() {
    extracted::<NotAPathInput>();
}
