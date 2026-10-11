//! A transparent variant answers as the one error it holds; a second field
//! leaves it unsaid which.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("{0}")]
    #[problem(transparent)]
    Lookup(LookupError, u64),
}

fn main() {}
