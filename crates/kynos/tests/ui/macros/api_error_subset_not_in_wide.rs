//! A subset converts into the wider enum by moving each variant's fields into
//! the wider variant of the same name, so a variant the wider enum lacks, a
//! field it lacks or has beside the subset's, and a field of another type are
//! each refused where the subset names them.
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
        #[problem(extension)]
        offset: u64,
    },

    #[error("the chunk was written concurrently")]
    #[problem(status = 409)]
    Conflict(u64),
}

#[derive(Debug, ApiError)]
#[problem(subset_of = ChunkRejection)]
enum UnknownVariant {
    Gone,
}

#[derive(Debug, ApiError)]
#[problem(subset_of = ChunkRejection)]
enum UnknownField {
    NotFound { id: u64, offset: u64, length: u64 },
}

#[derive(Debug, ApiError)]
#[problem(subset_of = ChunkRejection)]
enum MissingField {
    NotFound { id: u64 },
}

#[derive(Debug, ApiError)]
#[problem(subset_of = ChunkRejection)]
enum OtherType {
    Conflict(String),
}

fn main() {}
