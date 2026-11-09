//! The control for `traits/authorizer.rs`.
//!
//! Differs in exactly the property under test: the authenticator also
//! authorizes the scheme's scopes.
use kynos::{
    error::rejection::AuthRejection,
    security::{
        Authenticates, Authenticator, Authorizer, Scope, carrier::BearerToken, schemes::Bearer,
    },
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

impl<C: Sync> Authorizer<Bearer<String, Grant>, C, Grant> for Tokens {
    async fn authorize(&self, _: &String, _: &[Grant], _: &C) -> Result<(), AuthRejection> {
        Err(AuthRejection::forbidden())
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
