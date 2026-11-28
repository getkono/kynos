//! A transparent enum displays as the error each variant holds, so a
//! `thiserror` `#[error(...)]` beside it would implement `Display` twice, and a
//! variant saying `transparent` again says nothing.
//!
//! The control is `SharedWhole` in `tests/derives.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(transparent)]
enum BesideThiserror {
    #[error(transparent)]
    Lookup(LookupError),
}

#[derive(Debug, ApiError)]
#[problem(transparent)]
enum SaidTwice {
    #[problem(transparent)]
    Lookup(LookupError),
}

fn main() {}
