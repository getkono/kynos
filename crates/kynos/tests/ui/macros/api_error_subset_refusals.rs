//! A subset answers as the wider enum's variants, so it declares nothing they
//! already declare: no key beside `subset_of`, no list on a variant or a
//! field. `subset_of` belongs on an enum, and cannot name the subset's own
//! generic parameter, since its statuses are a `const`.
//!
//! The controls are the `a_subset_*` cases in `tests/derives.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum ChunkRejection {
    #[error("no chunk {id}")]
    #[problem(status = 404)]
    NotFound {
        #[problem(extension)]
        id: u64,
    },

    #[error("the chunk was written concurrently")]
    #[problem(status = 409)]
    Conflict,
}

#[derive(Debug, ApiError)]
enum OnAVariant {
    #[problem(subset_of = ChunkRejection)]
    Conflict,
}

#[derive(Debug, ApiError)]
#[problem(subset_of = ChunkRejection)]
struct OnAStruct;

#[derive(Debug, ApiError)]
#[problem(subset_of = ChunkRejection, base = "https://errors.example.com/")]
enum WithABase {
    Conflict,
}

#[derive(Debug, ApiError)]
#[problem(subset_of = Wrapped<T>)]
enum Generic<T> {
    Held(T),
}

#[derive(Debug, ApiError)]
#[problem(subset_of = ChunkRejection)]
enum RestatesAStatus {
    #[problem(status = 409)]
    Conflict,
}

#[derive(Debug, ApiError)]
#[problem(subset_of = ChunkRejection)]
enum RemarksAField {
    NotFound {
        #[problem(extension)]
        id: u64,
    },
}

fn main() {}
