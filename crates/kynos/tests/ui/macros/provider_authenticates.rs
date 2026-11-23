//! `authenticates` keeps a field back unless `provided` opts it in again, so
//! the combinations saying two things at once are refused, as is a scheme
//! given a second authenticator.
//!
//! The schemes are written in full rather than imported: a refused derive
//! emits nothing that would use the import.

struct Gate;

#[derive(kynos::Provider)]
struct Contradicts {
    #[provide(skip, provided, authenticates = kynos::security::schemes::Bearer)]
    gate: Gate,
}

#[derive(kynos::Provider)]
struct Redundant {
    #[provide(skip, authenticates = kynos::security::schemes::Bearer)]
    gate: Gate,
}

#[derive(Clone)]
struct Pool;

#[derive(kynos::Provider)]
struct OptsInNothing {
    #[provide(provided)]
    pool: Pool,
}

#[derive(kynos::Provider)]
struct NamedTwice {
    #[provide(
        authenticates = kynos::security::schemes::Bearer,
        authenticates = kynos::security::schemes::Basic
    )]
    #[provide(authenticates = kynos::security::schemes::Bearer)]
    gate: Gate,
}

#[derive(kynos::Provider)]
struct TwoAuthenticators {
    #[provide(authenticates = kynos::security::schemes::Bearer)]
    gate: Gate,
    #[provide(authenticates = kynos::security::schemes::Bearer)]
    fallback: Gate,
}

fn main() {}
