//! `from` builds a transparent variant from the one error it holds, so it is
//! refused on a variant declaring a failure of its own, on an enum that is not
//! transparent whole, beside `thiserror`'s `#[from]`, which implements the
//! same `From`, and on two variants holding one type, which one `From` cannot
//! build both of.
//!
//! The controls are `ConvertedPause`, `SharedFrom` and `PartlyFrom` in
//! `tests/derives.rs`.

use kynos::ApiError;

#[derive(Debug, thiserror::Error, ApiError)]
#[error("no user with that id")]
#[problem(status = 404)]
struct LookupError;

#[derive(Debug, thiserror::Error, ApiError)]
enum OnAFailureOfItsOwn {
    #[error("the store failed")]
    #[problem(status = 503, from)]
    Store(std::io::Error),
}

#[derive(Debug, thiserror::Error, ApiError)]
#[problem(from)]
enum NotTransparentWhole {
    #[error(transparent)]
    Lookup(LookupError),
}

#[derive(Debug, thiserror::Error, ApiError)]
enum BesideThiserrorsFrom {
    #[error(transparent)]
    #[problem(from)]
    Lookup(#[from] LookupError),
}

#[derive(Debug, ApiError)]
#[problem(transparent, from)]
enum OneTypeTwice {
    User(LookupError),
    Tenant(LookupError),
}

fn main() {}
