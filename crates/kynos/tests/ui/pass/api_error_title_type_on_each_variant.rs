//! The control for `macros/api_error_title_type_on_the_enum`: the same
//! `title` and `type`, each on the variant whose failure it names.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum TitledError {
    #[error("this item is gone")]
    #[problem(status = 410, title = "The store failed")]
    Gone,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum TypedError {
    #[error("this item is gone")]
    #[problem(status = 410, type = "https://errors.example.com/store")]
    Gone,
}

fn main() {}
