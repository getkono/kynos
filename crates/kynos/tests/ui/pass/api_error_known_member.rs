//! The control for `macros/api_error_unknown_member`: the same error, differing
//! only in that the member is spelled the way the grammar defines it.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404, title = "User not found")]
    NotFound,
}

fn main() {}
