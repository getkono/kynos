//! The control for `macros/api_error_extension_without_a_schema` and
//! `macros/api_error_extension_without_serialize`: the same member, differing
//! only in that its type both has a schema and serializes.

use kynos::{ApiError, Schema};

#[derive(Debug, Schema, serde::Serialize)]
struct Quota(u64);

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("over quota")]
    #[problem(status = 507)]
    OverQuota {
        #[problem(extension)]
        limit: Quota,
    },
}

fn main() {}
