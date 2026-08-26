//! The control for `macros/api_error_without_display`: the same error,
//! differing only in that it has the `Display` its detail comes from.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
