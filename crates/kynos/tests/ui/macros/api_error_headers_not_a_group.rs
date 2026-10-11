//! A `headers` field holds a header group, so a field of any other type has no
//! headers to send or declare.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum SessionError {
    #[error("too many sessions")]
    #[problem(status = 429)]
    Throttled {
        #[problem(headers)]
        retry_after: u64,
    },
}

fn main() {}
