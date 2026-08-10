//! The control for `macros/api_error_status_on_the_enum`: the same shape,
//! differing only in that what the type declares is the one member every
//! variant genuinely shares.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
