//! A transparent enum is an `Error` whose source is the held error's, so one
//! written beside it implements `Error` a second time, which the compiler
//! reports at both. The derive cannot see an `impl` elsewhere; `thiserror`'s
//! `#[error(...)]` it refuses itself, in `api_error_transparent_enum_refusals`.
//!
//! The control is `SharedSourced` in `tests/derives.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, ApiError)]
#[problem(transparent)]
enum WrittenTwice {
    Lookup(LookupError),
}

impl std::error::Error for WrittenTwice {}

fn main() {}
