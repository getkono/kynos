//! Beside a transparent variant, a status `describe` names may be one only the
//! held error answers with, which the derive cannot read; a `const` checks
//! it instead, and refuses a status neither answers with at the status.
//!
//! The control is `a_written_description_covers_what_a_transparent_variant_answers`
//! in `tests/description.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum OriginRefused {
    #[error("cross-origin request rejected")]
    #[problem(status = 403)]
    CrossOrigin,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(describe(409 = "The origin conflicts"))]
enum GatewayRefused {
    #[error("the gateway is closed")]
    #[problem(status = 503)]
    Closed,

    #[error(transparent)]
    Origin(OriginRefused),
}

fn main() {}
