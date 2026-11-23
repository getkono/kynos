//! `#[error(transparent)]` makes a variant declaring no status answer as the
//! error it holds, so a title beside it would be dropped as that error's. The
//! control is `pass/api_error_transparent_variant`, whose opt-out declares a
//! status beside its title.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error(transparent)]
    #[problem(title = "User not found")]
    Lookup(#[from] LookupError),
}

fn main() {}
