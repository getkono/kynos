//! A transparent variant answers as the error it holds, so a status of its own
//! would contradict the one that error declares.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error(transparent)]
    #[problem(transparent, status = 404)]
    Lookup(#[from] LookupError),
}

fn main() {}
