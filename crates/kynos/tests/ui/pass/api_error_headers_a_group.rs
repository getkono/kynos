//! The control for `macros/api_error_headers_not_a_group`: the `headers` field
//! holds a derived header group.

use kynos::{ApiError, HeaderParams};

#[derive(Debug, HeaderParams)]
struct RetryAfter {
    #[header(rename = "Retry-After")]
    retry_after: u64,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum SessionError {
    #[error("too many sessions")]
    #[problem(status = 429)]
    Throttled {
        #[problem(headers)]
        retry_after: RetryAfter,
    },
}

fn main() {}
