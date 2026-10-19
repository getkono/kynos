//! A field's `#[problem(...)]` markers are bare and written once, and a field
//! is either an extension member or a header group. A failure sends one
//! header group, so a second is refused rather than merged.
//!
//! The controls are `pass/api_error_extension_with_a_name.rs` and
//! `pass/api_error_headers_a_group.rs`: one marker on each field, and one
//! group.

use kynos::{ApiError, HeaderParams};

#[derive(Debug, HeaderParams)]
struct RetryAfter {
    #[header(rename = "Retry-After")]
    retry_after: u64,
}

#[derive(Debug, HeaderParams)]
struct RateLimit {
    #[header(rename = "RateLimit")]
    limit: u64,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound {
        #[problem(extension)]
        #[problem(extension)]
        id: u64,
    },
}

#[derive(Debug, thiserror::Error, ApiError)]
enum LookupError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound {
        #[problem(extension = "user_id")]
        id: u64,
    },
}

#[derive(Debug, thiserror::Error, ApiError)]
enum SessionError {
    #[error("too many sessions")]
    #[problem(status = 429)]
    Throttled {
        #[problem(extension, headers)]
        retry: RetryAfter,
    },
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("too many requests")]
#[problem(status = 429)]
struct Throttled {
    #[problem(headers)]
    retry: RetryAfter,
    #[problem(headers)]
    limit: RateLimit,
}

fn main() {}
