//! An extension member is published under its field's name, so a tuple variant
//! has nothing to publish it as.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with id {0}")]
    #[problem(status = 404)]
    NotFound(#[problem(extension)] u64),
}

fn main() {}
