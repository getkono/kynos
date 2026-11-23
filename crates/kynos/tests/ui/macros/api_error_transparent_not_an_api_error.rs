//! A transparent variant answers as the `ApiError` it holds, so a field holding
//! any other error has no statuses to contribute, whether `#[problem]` says
//! `transparent` or `#[error(transparent)]` implies it.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error(transparent)]
    #[problem(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error, ApiError)]
enum ParseError {
    #[error(transparent)]
    Number(#[from] std::num::ParseIntError),
}

fn main() {}
