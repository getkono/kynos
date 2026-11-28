//! A subset of several enums answers as the first it names, so a variant
//! answering with other statuses in a later one is refused in a `const`:
//! converted into either, one failure cannot change what it declares.
//!
//! The control is `SharedChunkLookup` in `tests/derives.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum PutError {
    #[error("no such title")]
    #[problem(status = 404)]
    TitleUnavailable,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum LogPlaysError {
    #[error("that title is gone")]
    #[problem(status = 410)]
    TitleUnavailable,
}

#[derive(Debug, ApiError)]
#[problem(subset_of = (PutError, LogPlaysError))]
enum TitleError {
    TitleUnavailable,
}

fn main() {}
