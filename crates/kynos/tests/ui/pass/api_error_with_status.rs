//! The control for `macros/api_error_missing_status`: the same error, differing
//! only in that the variant declares its status.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
