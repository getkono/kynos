//! `include` marks the variant holding a shared enum, so it is refused on the
//! type, which a derive cannot add the variant to, and with a value; beside
//! the `from`, `variant_of` or `#[from]` it overlaps or contradicts; in an
//! enum saying `from`; beside a `from` variant holding its type; and twice in
//! one enum, whose two generic `From`s overlap.
//!
//! The controls are `RegisterError` and `IncludedWhole` in
//! `tests/derives.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
enum ServerError {
    #[error("the server failed")]
    #[problem(status = 500)]
    Internal,
}

#[derive(Debug, thiserror::Error, ApiError)]
#[error("the store failed")]
#[problem(status = 503)]
struct StoreError;

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(include = ServerError)]
enum OnTheEnum {
    #[error("the name is taken")]
    #[problem(status = 409)]
    Conflict,
}

#[derive(Debug, thiserror::Error, ApiError)]
enum WithAValue {
    #[error(transparent)]
    #[problem(include = ServerError)]
    Server(ServerError),
}

#[derive(Debug, ApiError)]
#[problem(transparent, from)]
enum InAnEnumSayingFrom {
    Store(StoreError),
    #[problem(include)]
    Server(ServerError),
}

#[derive(Debug, thiserror::Error, ApiError)]
enum BesideFrom {
    #[error(transparent)]
    #[problem(include, from)]
    Server(ServerError),
}

#[derive(Debug, thiserror::Error, ApiError)]
enum BesideVariantOf {
    #[error(transparent)]
    #[problem(include, variant_of = Wide::Server)]
    Server(ServerError),
}

#[derive(Debug, thiserror::Error, ApiError)]
enum BesideThiserrorsFrom {
    #[error(transparent)]
    #[problem(include)]
    Server(#[from] ServerError),
}

#[derive(Debug, thiserror::Error, ApiError)]
enum BesideAFromOfItsType {
    #[error(transparent)]
    #[problem(from)]
    Fault(ServerError),
    #[error(transparent)]
    #[problem(include)]
    Server(ServerError),
}

#[derive(Debug, thiserror::Error, ApiError)]
enum Twice {
    #[error(transparent)]
    #[problem(include)]
    Server(ServerError),
    #[error(transparent)]
    #[problem(include)]
    Store(StoreError),
}

fn main() {}
