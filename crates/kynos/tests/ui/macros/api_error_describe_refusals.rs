//! `describe` writes what a whole enum says of a status, so it belongs on the
//! enum, never on a variant or on a struct, whose `title` already says it. It
//! names at least one status, each once, each with something to say, and only
//! statuses a variant answers with.
//!
//! The controls are the `a_written_description_*` cases in
//! `tests/description.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum OnAVariant {
    #[error("cross-origin request rejected")]
    #[problem(status = 403, describe(403 = "The request's origin is not allowed"))]
    CrossOrigin,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("cross-origin request rejected")]
#[problem(status = 403, describe(403 = "The request's origin is not allowed"))]
struct OnAStruct;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(describe())]
enum NamesNoStatus {
    #[error("cross-origin request rejected")]
    #[problem(status = 403)]
    CrossOrigin,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(describe(403 = "The request's origin is not allowed", 403 = "Refused"))]
enum DescribedTwice {
    #[error("cross-origin request rejected")]
    #[problem(status = 403)]
    CrossOrigin,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(describe(403 = " "))]
enum SaysNothing {
    #[error("cross-origin request rejected")]
    #[problem(status = 403)]
    CrossOrigin,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(describe(409 = "The origin conflicts"))]
enum Unanswered {
    #[error("cross-origin request rejected")]
    #[problem(status = 403)]
    CrossOrigin,
}

fn main() {}
