//! The control for `macros/api_error_transparent_generic`: a generic enum
//! whose transparent variant holds a concrete error type.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError<E: std::fmt::Debug> {
    #[error(transparent)]
    #[problem(transparent)]
    Lookup(LookupError),

    #[error("the record is gone")]
    #[problem(status = 410)]
    Gone(std::marker::PhantomData<E>),
}

fn main() {}
