//! `base` is joined with each slug as a string, so a value that is neither a
//! string literal nor a path to a `const` holding one is refused at the derive.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = 42)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
