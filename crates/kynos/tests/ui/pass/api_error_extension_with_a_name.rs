//! The control for `macros/api_error_extension_without_a_name`: the same
//! variant, differing only in that the published field has a name.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with id {id}")]
    #[problem(status = 404)]
    NotFound {
        #[problem(extension)]
        id: u64,
    },
}

fn main() {}
