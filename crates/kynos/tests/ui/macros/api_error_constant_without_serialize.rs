//! A constant extension member is sent through its value's `Serialize`, so a
//! `const` whose type has none is refused where the variant names it.

use kynos::ApiError;

#[derive(Debug)]
struct Code(&'static str);

const NOT_FOUND: Code = Code("error.store.not_found");

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("not found")]
    #[problem(status = 404, extension(code = NOT_FOUND))]
    NotFound,
}

fn main() {}
