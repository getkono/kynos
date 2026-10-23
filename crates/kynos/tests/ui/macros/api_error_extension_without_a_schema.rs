//! An extension member is declared by its type's schema, so a type that
//! reaches the wire but has none is refused where the field names it.

use kynos::ApiError;

#[derive(Debug)]
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
