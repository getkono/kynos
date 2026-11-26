//! A derived error naming no type URI sends `about:blank`, which names no
//! problem type for a refusal to share.

use kynos::{ApiError, error::problem::refusal_problem, http::StatusCode};

#[derive(Debug, thiserror::Error, ApiError)]
#[error("the quota is spent")]
#[problem(status = 429)]
struct QuotaExceeded;

fn main() {
    let _ = refusal_problem::<QuotaExceeded>(StatusCode::TOO_MANY_REQUESTS);
}
