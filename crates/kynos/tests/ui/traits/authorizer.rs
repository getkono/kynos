//! A context authenticating a scheme with scopes through an authenticator that
//! never says how it authorizes them.
use kynos::{
    error::rejection::AuthRejection,
    security::{Authenticates, Authenticator, Scope, carrier::BearerToken, schemes::Bearer},
};

#[derive(Clone, Copy)]
enum Grant {
    ReadReports,
}

impl Scope for Grant {
    fn name(&self) -> &'static str {
        match self {
            Self::ReadReports => "reports:read",
        }
    }
}

struct Tokens;

impl<C: Sync> Authenticator<Bearer<String, Grant>, C> for Tokens {
    async fn authenticate(&self, _: BearerToken, _: &C) -> Result<String, AuthRejection> {
        Err(AuthRejection::unauthenticated())
    }
}

struct App;

impl Authenticates<Bearer<String, Grant>> for App {
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Tokens {
        &Tokens
    }
}

fn main() {}
