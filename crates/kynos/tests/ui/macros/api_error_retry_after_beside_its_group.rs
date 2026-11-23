//! A failure's `retry_after` field sends `Retry-After`, so a header group
//! beside it naming that field too is refused rather than left to overwrite
//! one with the other.
//!
//! The control is `pass/api_error_retry_after_a_delay.rs`, whose group beside
//! the delay names another field.

use kynos::{ApiError, HeaderParams};

#[derive(Debug, HeaderParams)]
struct Throttling {
    #[header(rename = "Retry-After")]
    retry_after: u64,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("too many sessions")]
#[problem(status = 429)]
struct Throttled {
    #[problem(retry_after)]
    wait: std::time::Duration,
    #[problem(headers)]
    throttling: Throttling,
}

fn main() {}
