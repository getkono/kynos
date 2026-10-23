//! The control for `macros/api_error_extension_without_a_schema`: the same
//! member, differing only in that its type has a schema.

use kynos::{ApiError, Schema};

#[derive(Debug, Schema)]
struct Quota(u64);

impl From<Quota> for serde_json::Value {
    fn from(quota: Quota) -> Self {
        quota.0.into()
    }
}

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
