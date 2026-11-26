//! The control for `macros/api_error_problem_type_written_twice`: the same
//! hand-written `ProblemType` on a struct naming no type URI, which the derive
//! leaves to it.

use kynos::{ApiError, error::problem::ProblemType};

#[derive(Debug, thiserror::Error, ApiError)]
#[error("the quota is spent")]
#[problem(status = 429)]
struct QuotaExceeded;

impl ProblemType for QuotaExceeded {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/quota-exceeded");
}

fn main() {}
