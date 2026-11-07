//! An extension member is declared by its type's schema, so a type that
//! reaches the wire but has none is refused where the field names it.

use kynos::ApiError;

#[derive(Debug, serde::Serialize)]
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
