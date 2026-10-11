//! A `base` path is read as a `&str`, so a `const` of any other type fails
//! type-checking, reported at the path `base` names rather than inside the
//! expansion. The control is `pass/api_error_base_from_a_const`, whose `const`
//! differs only in being a `&str`.

use kynos::ApiError;

const PROBLEM_BASE: u16 = 42;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = PROBLEM_BASE)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
