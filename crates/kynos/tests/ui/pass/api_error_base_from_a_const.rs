//! The control for `macros/api_error_base_not_a_prefix`: the same error,
//! differing only in that `base` names a `const` holding the prefix.

use kynos::ApiError;

const PROBLEM_BASE: &str = "https://errors.example.com/";

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = PROBLEM_BASE)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
