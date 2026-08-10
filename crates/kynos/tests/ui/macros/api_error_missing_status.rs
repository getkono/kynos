//! A variant that does not say what status it produces would leave the
//! description guessing, so the derive refuses rather than choosing one.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with that id")]
    NotFound,
}

fn main() {}
