//! A `retry_after` marker is bare and goes on the one field holding the delay,
//! which is neither an extension member nor a header group. A failure sends
//! one delay, so a second is refused rather than either chosen, and a
//! transparent variant sends the delay of the error it holds.
//!
//! The control is `pass/api_error_retry_after_a_delay.rs`: one delay on each
//! failure, marked alone.

use std::time::Duration;

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum OnTheVariant {
    #[error("too many sessions")]
    #[problem(status = 429, retry_after)]
    Throttled { wait: Duration },
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("too many sessions")]
#[problem(status = 429)]
struct GivenAValue {
    #[problem(retry_after = 5)]
    wait: Duration,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("too many sessions")]
#[problem(status = 429)]
struct AlsoAnExtension {
    #[problem(extension, retry_after)]
    wait: Duration,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("too many sessions")]
#[problem(status = 429)]
struct Twice {
    #[problem(retry_after)]
    wait: Duration,
    #[problem(retry_after)]
    again: Duration,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum Transparent {
    #[error(transparent)]
    #[problem(transparent)]
    Lookup(#[problem(retry_after)] LookupError),
}

fn main() {}
