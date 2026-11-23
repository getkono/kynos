//! A `retry_after` field holds the delay `Retry-After` sends, so a field of any
//! other type, a count of seconds included, is refused at the field: the
//! derive rounds the delay itself.
//!
//! The control is `pass/api_error_retry_after_a_delay.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum SessionError {
    #[error("too many sessions")]
    #[problem(status = 429)]
    Throttled {
        #[problem(retry_after)]
        wait: u64,
    },
}

fn main() {}
