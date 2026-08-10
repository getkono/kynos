//! The control for `antipattern/problem_as_return_type`: the same `Result`
//! shape, differing only in that the error names its statuses instead of
//! carrying one in a field.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn returns<T: kynos::response::IntoResponse + kynos::response::Responses>() {}

fn main() {
    returns::<Result<kynos::response::status::NoContent, StoreError>>();
}
