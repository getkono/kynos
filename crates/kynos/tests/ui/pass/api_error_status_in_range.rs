//! The control for `macros/api_error_status_out_of_range`: the same error,
//! differing only in that the status names a failure.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("the upstream is unreachable")]
    #[problem(status = 502)]
    Upstream,
}

fn main() {}
