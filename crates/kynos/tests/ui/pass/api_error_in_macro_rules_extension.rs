//! An `extension` member compiles when a `macro_rules!` adds the derive and is
//! handed the field types: the derive's own bindings keep its hygiene context
//! however the member's type tokens arrive.

use kynos::{ApiError, Schema};

#[derive(Debug, Schema, serde::Serialize)]
struct Quota(u64);

macro_rules! app_error {
    (enum $name:ident { $($body:tt)* }) => {
        #[derive(Debug, thiserror::Error, ApiError)]
        #[problem(base = "https://example.com/problems#")]
        enum $name { $($body)* }
    };
}

app_error! {
    enum StoreError {
        #[error("over quota")]
        #[problem(status = 507, type = "over-quota")]
        OverQuota {
            #[problem(extension)]
            limit: Quota,
        },
    }
}

fn main() {}
