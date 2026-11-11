//! The control for `macros/api_error_constant_without_serialize`: the same
//! constant, differing only in that its type serializes. A `macro_rules!`
//! forwarding the value as an `expr` fragment is read as the path it holds.

use kynos::ApiError;

#[derive(Debug, serde::Serialize)]
struct Code(&'static str);

const NOT_FOUND: Code = Code("error.store.not_found");

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("not found")]
    #[problem(status = 404, extension(code = NOT_FOUND))]
    NotFound,
}

macro_rules! coded {
    ($name:ident, $code:expr) => {
        #[derive(Debug, thiserror::Error, ApiError)]
        #[error("gone")]
        #[problem(status = 410, extension(code = $code))]
        struct $name;
    };
}

coded!(Gone, NOT_FOUND);
coded!(Expired, "error.store.expired");

fn main() {}
