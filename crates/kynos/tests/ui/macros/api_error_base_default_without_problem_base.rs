//! `base = default` reads the crate's `kynos::problem_base!`, so a crate
//! declaring none is refused, reported at `default` rather than inside the
//! expansion. The control is `pass/api_error_base_default`, which differs only
//! in declaring the base.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = default)]
enum StoreError {
    #[error("no user with that id")]
    #[problem(status = 404)]
    NotFound,
}

fn main() {}
