//! A `title` or a `type` on the enum names no variant's failure, and no variant
//! publishes it, so each is refused rather than dropped, as `status` is.
//!
//! The control is `pass/api_error_title_type_on_each_variant.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(title = "The store failed")]
enum TitledError {
    #[error("this item is gone")]
    #[problem(status = 410)]
    Gone,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(type = "https://errors.example.com/store")]
enum TypedError {
    #[error("this item is gone")]
    #[problem(status = 410)]
    Gone,
}

fn main() {}
