//! A transparent variant's statuses are joined in a `const`, which cannot name
//! the enum's generic parameters.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError<E: std::error::Error + 'static> {
    #[error(transparent)]
    #[problem(transparent)]
    Lookup(E),
}

fn main() {}
