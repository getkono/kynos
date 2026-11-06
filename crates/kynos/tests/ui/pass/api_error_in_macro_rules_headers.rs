//! A `headers` field compiles when a `macro_rules!` adds the derive and is
//! handed the field types: the derive's own bindings keep its hygiene context
//! however the group's type tokens arrive.

use kynos::{ApiError, HeaderParams};

#[derive(Debug, HeaderParams)]
struct RetryAfter {
    #[header(rename = "Retry-After")]
    retry_after: u64,
}

macro_rules! app_error {
    (enum $name:ident { $($body:tt)* }) => {
        #[derive(Debug, thiserror::Error, ApiError)]
        #[problem(base = "https://example.com/problems#")]
        enum $name { $($body)* }
    };
}

app_error! {
    enum SessionError {
        #[error("too many sessions")]
        #[problem(status = 429, type = "throttled")]
        Throttled {
            #[problem(headers)]
            retry_after: RetryAfter,
        },
    }
}

fn main() {}
