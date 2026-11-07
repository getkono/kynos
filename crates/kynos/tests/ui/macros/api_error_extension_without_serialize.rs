//! An extension member is sent through its type's `Serialize`, so a type with
//! a schema but no `Serialize` is refused where the field names it, and a
//! hand-written `From<_> for serde_json::Value` does not stand in for it.

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
