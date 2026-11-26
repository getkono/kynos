//! The control for `traits/problem_type`: the same error under a `base`,
//! which names its type URI, so a refusal shares it.

use kynos::{ApiError, error::problem::refusal_problem, http::StatusCode};

#[derive(Debug, thiserror::Error, ApiError)]
#[error("the quota is spent")]
#[problem(base = "https://errors.example.com/", status = 429)]
struct QuotaExceeded;

fn main() {
    let _ = refusal_problem::<QuotaExceeded>(StatusCode::TOO_MANY_REQUESTS);
}
