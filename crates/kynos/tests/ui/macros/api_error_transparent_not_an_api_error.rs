//! A transparent variant answers as the `ApiError` it holds, so a field holding
//! any other error has no statuses to contribute.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error(transparent)]
    #[problem(transparent)]
    Io(#[from] std::io::Error),
}

fn main() {}
