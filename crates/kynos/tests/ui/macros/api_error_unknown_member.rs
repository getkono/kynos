//! The grammar is closed, so a misspelled member is an error rather than a
//! silently dropped title.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404, titel = "User not found")]
    NotFound,
}

fn main() {}
