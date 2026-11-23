//! A `variant_of` variant builds the wider variant from its fields, by name,
//! so a variant the wider enum lacks, and a field it lacks or has beside the
//! variant's, are each refused where they are named.
//!
//! The controls are the `a_variant_of_variant_*` cases in `tests/derives.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum ProfileProblem {
    #[error("the profile is unavailable: {detail}")]
    #[problem(status = 503)]
    Unavailable { detail: String, retry: u64 },
}

#[derive(Debug, thiserror::Error, ApiError)]
enum UnknownVariant {
    #[error("gone")]
    #[problem(variant_of = ProfileProblem::Gone)]
    Gone,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum UnknownField {
    #[error("unavailable")]
    #[problem(variant_of = ProfileProblem::Unavailable)]
    Unavailable {
        detail: String,
        retry: u64,
        reason: String,
    },
}

#[derive(Debug, thiserror::Error, ApiError)]
enum MissingField {
    #[error("unavailable")]
    #[problem(variant_of = ProfileProblem::Unavailable)]
    Unavailable { detail: String },
}

fn main() {}
