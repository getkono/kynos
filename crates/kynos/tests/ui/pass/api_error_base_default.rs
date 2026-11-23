//! The control for `macros/api_error_base_default_without_problem_base`: the
//! same error, differing only in that the crate declares its problem base.

use kynos::ApiError;

kynos::problem_base!("https://errors.example.com/");

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = default)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
