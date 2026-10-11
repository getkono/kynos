//! `transparent` hands one variant to the error it holds; a struct has no
//! variant to hand over.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error(transparent)]
#[problem(transparent)]
struct StoreError(LookupError);

fn main() {}
