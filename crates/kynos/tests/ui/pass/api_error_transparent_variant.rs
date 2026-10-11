//! The control for `macros/api_error_transparent_on_the_type`,
//! `_with_a_status`, `_with_two_fields` and `_extension`: `transparent` on a
//! variant of an enum, declaring nothing else, over one field that publishes
//! nothing of its own.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error(transparent)]
    #[problem(transparent)]
    Lookup {
        #[from]
        source: LookupError,
    },
}

fn main() {}
