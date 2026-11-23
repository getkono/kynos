//! The control for `macros/provider_authenticates`: the same contexts, each
//! saying one thing, every scheme with one authenticator.

use kynos::{
    error::rejection::AuthRejection,
    security::{
        Authenticator, Head, carrier::BearerToken, carrier::Credentials,
        carrier::presented::Presented, schemes::Basic, schemes::Bearer,
    },
};

#[derive(Clone)]
struct Gate;

impl<C: Sync> Authenticator<Bearer, C> for Gate {
    async fn authenticate(
        &self,
        _: Presented<BearerToken>,
        _: Head<'_>,
        _: &C,
    ) -> Result<String, AuthRejection> {
        Err(AuthRejection::unauthenticated())
    }
}

impl<C: Sync> Authenticator<Basic, C> for Gate {
    async fn authenticate(
        &self,
        _: Presented<Credentials>,
        _: Head<'_>,
        _: &C,
    ) -> Result<Credentials, AuthRejection> {
        Err(AuthRejection::unauthenticated())
    }
}

#[derive(kynos::Provider)]
struct Contradicts {
    #[provide(provided, authenticates = Bearer)]
    gate: Gate,
}

#[derive(kynos::Provider)]
struct Redundant {
    #[provide(authenticates = Bearer)]
    gate: Gate,
}

#[derive(Clone)]
struct Pool;

#[derive(kynos::Provider)]
struct OptsInNothing {
    pool: Pool,
}

#[derive(kynos::Provider)]
struct NamedTwice {
    #[provide(authenticates = Bearer, authenticates = Basic)]
    gate: Gate,
}

#[derive(kynos::Provider)]
struct TwoAuthenticators {
    #[provide(authenticates = Bearer)]
    gate: Gate,
    fallback: Gate,
}

fn main() {}
