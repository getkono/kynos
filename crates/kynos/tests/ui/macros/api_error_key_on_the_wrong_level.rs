//! Each `#[problem(...)]` key belongs where what it says lives: `base` on the
//! type, as the prefix its variants share; `extension` on the field it
//! publishes; and `headers` on the field holding the group.
//!
//! The controls are `pass/api_error_base_on_the_enum.rs`,
//! `pass/api_error_extension_with_a_name.rs` and
//! `pass/api_error_headers_a_group.rs`.

use kynos::{ApiError, HeaderParams};

#[derive(Debug, HeaderParams)]
struct RetryAfter {
    #[header(rename = "Retry-After")]
    retry_after: u64,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404, base = "https://example.com/")]
    NotFound,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404, extension)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum SessionError {
    #[error("too many sessions")]
    #[problem(status = 429, headers = RetryAfter)]
    Throttled { retry: RetryAfter },
}

fn main() {}
