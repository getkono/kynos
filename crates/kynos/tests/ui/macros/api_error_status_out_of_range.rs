//! A problem detail describes a failure, so a 2xx has nothing to describe.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("all is well")]
    #[problem(status = 200)]
    Fine,
}

fn main() {}
