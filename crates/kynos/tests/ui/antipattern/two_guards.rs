//! An operation takes one guard: two credential arguments are not a handler.
//!
//! Each would run, so the server would demand both, while each would declare
//! its own security requirement, which OpenAPI reads as "either". Combining
//! schemes is spelled in the guard's type parameter instead.

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

async fn both(key: Auth<ServiceKey>, token: Auth<Bearer>) -> NoContent {
    let _ = (key, token);
    NoContent
}

fn is_handler<C, A, H: kynos::handler::Handler<C, A>>(_: H) {}

fn main() {
    is_handler::<App, _, _>(both);
}
