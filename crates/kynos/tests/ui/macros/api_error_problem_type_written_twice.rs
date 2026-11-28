//! A struct naming a type URI already is the `ProblemType` of it, so one
//! written by hand beside the derive's conflicts with it.

use kynos::{ApiError, error::problem::ProblemType};

#[derive(Debug, thiserror::Error, ApiError)]
#[error("the quota is spent")]
#[problem(base = "https://errors.example.com/", status = 429)]
struct QuotaExceeded;

impl ProblemType for QuotaExceeded {
    const TYPE_URI: Option<&'static str> = Some("https://errors.example.com/quota-exceeded");
}

fn main() {}
