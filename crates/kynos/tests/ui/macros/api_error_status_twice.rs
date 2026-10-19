//! A response has one status, so a failure declaring a second would leave the
//! description to pick one; the derive refuses it on the type and on a variant.
//!
//! The control is `pass/api_error_with_status.rs`: one status.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no such store")]
#[problem(status = 404, status = 410)]
struct StoreGone;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    #[problem(status = 410)]
    NotFound,
}

fn main() {}
