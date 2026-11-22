//! A `variant_of` variant answers as the wider variant, so it declares nothing
//! that variant already declares: no key beside `variant_of`, no list on a
//! field. It belongs on a variant, names that variant through its enum, cannot
//! name the enum's own generic parameter, since its statuses are a `const`,
//! and takes a wider variant once, since a wider value converts back into one
//! variant.
//!
//! The controls are the `a_variant_of_variant_*` cases in `tests/derives.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum ProfileProblem {
    #[error("the profile is unavailable: {detail}")]
    #[problem(status = 503)]
    Unavailable {
        detail: String,
        #[problem(extension)]
        profile_status: String,
    },
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("unavailable")]
#[problem(variant_of = ProfileProblem::Unavailable)]
struct OnTheType;

#[derive(Debug, thiserror::Error, ApiError)]
enum WithAStatus {
    #[error("unavailable")]
    #[problem(variant_of = ProfileProblem::Unavailable, status = 503)]
    Unavailable {
        detail: String,
        profile_status: String,
    },
}

#[derive(Debug, thiserror::Error, ApiError)]
enum WithAMarker {
    #[error("unavailable")]
    #[problem(variant_of = ProfileProblem::Unavailable)]
    Unavailable {
        detail: String,
        #[problem(extension)]
        profile_status: String,
    },
}

#[derive(Debug, thiserror::Error, ApiError)]
enum NoEnum {
    #[error("unavailable")]
    #[problem(variant_of = Unavailable)]
    Unavailable,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum Generic<T: std::fmt::Debug> {
    #[error("unavailable")]
    #[problem(variant_of = Wide::<T>::Gone)]
    Gone(T),
}

#[derive(Debug, thiserror::Error, ApiError)]
enum TakenTwice {
    #[error("unavailable")]
    #[problem(variant_of = ProfileProblem::Unavailable)]
    Unavailable {
        detail: String,
        profile_status: String,
    },

    #[error("down")]
    #[problem(variant_of = ProfileProblem::Unavailable)]
    Down {
        detail: String,
        profile_status: String,
    },
}

fn main() {}
