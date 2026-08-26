//! A problem's `detail` comes from `Display`, so an error without one would
//! describe every occurrence identically.

use kynos::ApiError;

#[derive(Debug, ApiError)]
enum StoreError {
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
