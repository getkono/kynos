//! The control for `macros/api_error_transparent_on_the_type`,
//! `_with_a_status`, `_with_two_fields`, `_extension` and
//! `macros/api_error_error_transparent_with_a_title`: `transparent` on a
//! variant of an enum, declaring nothing else, over one field that publishes
//! nothing of its own; the same implied by `#[error(transparent)]` alone; and
//! a status beside a title opting a variant `#[error(transparent)]` displays
//! out of transparency.

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

#[derive(Debug, thiserror::Error, ApiError)]
enum ImpliedError {
    #[error(transparent)]
    Lookup(#[from] LookupError),
}

#[derive(Debug, thiserror::Error)]
#[error("row missing")]
struct RowMissing;

#[derive(Debug, thiserror::Error, ApiError)]
enum OptedOutError {
    #[error(transparent)]
    #[problem(status = 404, title = "User not found")]
    Missing(#[from] RowMissing),
}

fn main() {}
