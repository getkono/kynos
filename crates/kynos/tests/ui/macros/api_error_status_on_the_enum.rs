//! A status on the enum would apply to every variant, which is the opposite of
//! what a closed set of failures is for.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(status = 404)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
