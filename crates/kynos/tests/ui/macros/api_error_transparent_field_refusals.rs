//! A transparent variant answers as the error it holds answers, so its field
//! sends that error's headers rather than a group of its own, and its type
//! cannot name the enum's generic parameters, a lifetime included, since the
//! held error's description is read without them.
//!
//! The controls are `pass/api_error_transparent_variant.rs` and
//! `pass/api_error_transparent_concrete.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error(transparent)]
    #[problem(transparent)]
    Lookup(#[problem(headers)] LookupError),
}

#[derive(Debug, thiserror::Error)]
#[error("wrapped")]
struct Wrapped<T: std::fmt::Debug>(T);

#[derive(Debug, thiserror::Error, ApiError)]
enum BorrowedError<'a, E: std::fmt::Debug> {
    #[error(transparent)]
    #[problem(transparent)]
    Lookup(Wrapped<&'a E>),
}

fn main() {}
