//! The control for `antipattern/two_guards.rs`.
//!
//! Differs in exactly the property under test: the operation takes one guard.

use kynos::{
    error::rejection::AuthRejection,
    response::status::NoContent,
    security::{Authenticates, Authenticator, auth::Auth, carrier::Carries, schemes::Bearer},
};

#[derive(kynos::SecurityScheme)]
#[security(api_key(in = "header", name = "X-Api-Key"))]
#[security(credential = String)]
struct ServiceKey;

struct Verifier;

impl<S: Carries<Credential = String>, C: Sync> Authenticator<S, C> for Verifier {
    async fn authenticate(&self, _: S::Presented, _: &C) -> Result<String, AuthRejection> {
        Err(AuthRejection::unauthenticated())
    }

    async fn authorize(
        &self,
        _: &String,
        _: &'static [&'static str],
        _: &C,
    ) -> Result<(), AuthRejection> {
        Ok(())
    }
}

struct App;

impl Authenticates<ServiceKey> for App {
    type Authenticator = Verifier;

    fn authenticator(&self) -> &Verifier {
        &Verifier
    }
}

impl Authenticates<Bearer> for App {
    type Authenticator = Verifier;

    fn authenticator(&self) -> &Verifier {
        &Verifier
    }
}

async fn one(token: Auth<Bearer>) -> NoContent {
    let _ = token;
    NoContent
}

fn is_handler<C, A, H: kynos::handler::Handler<C, A>>(_: H) {}

fn main() {
    is_handler::<App, _, _>(one);
}
