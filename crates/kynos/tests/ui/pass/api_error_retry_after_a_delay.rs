//! The control for `macros/api_error_retry_after_not_a_delay`,
//! `macros/api_error_retry_after_beside_its_group` and
//! `macros/api_error_retry_after_refusals`: one `retry_after` field per
//! failure, marked alone, holding a `Duration` or an `Option` of one, on a
//! named and a tuple variant, and beside a group naming another field.

use std::time::Duration;

use kynos::{ApiError, HeaderParams};

#[derive(Debug, HeaderParams)]
struct RateLimit {
    #[header(rename = "RateLimit")]
    limit: u64,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum SessionError {
    #[error("too many sessions")]
    #[problem(status = 429)]
    Throttled {
        #[problem(retry_after)]
        wait: Duration,
        #[problem(headers)]
        limit: RateLimit,
    },

    #[error("sessions are paused")]
    #[problem(status = 503)]
    Paused(#[problem(retry_after)] Option<Duration>),
}

fn main() {}
