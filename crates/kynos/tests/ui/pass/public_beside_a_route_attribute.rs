//! The control for `macros/public_refusals.rs` and
//! `antipattern/public_on_a_guarded_handler.rs`.
//!
//! Differs in exactly the properties under test: `#[kynos::public]` is
//! written once, without arguments, beside a route attribute, above it and
//! below it, on handlers taking no guard, mounted beside one that takes one.

use kynos::{
    Router,
    error::rejection::AuthRejection,
    response::status::NoContent,
    router::group::Group,
    security::{
        Authenticates, Authenticator, Head, auth::Auth, carrier::BearerToken,
        carrier::presented::Presented, schemes::Bearer,
    },
};

struct Tokens;

impl<C: Sync> Authenticator<Bearer, C> for Tokens {
    async fn authenticate(
        &self,
        _: Presented<BearerToken>,
        _: Head<'_>,
        _: &C,
    ) -> Result<String, AuthRejection> {
        Err(AuthRejection::unauthenticated())
    }
}

struct App;

impl Authenticates<Bearer> for App {
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Tokens {
        &Tokens
    }
}

#[kynos::delete("/sessions/current")]
async fn revoke_session(caller: Auth<Bearer>) -> NoContent {
    let _ = caller;
    NoContent
}

#[kynos::public]
#[kynos::post("/sessions")]
async fn create_session() -> NoContent {
    NoContent
}

#[kynos::get("/status")]
#[kynos::public]
async fn status() -> NoContent {
    NoContent
}

fn main() {
    let _ = Router::<App>::new().group(Group::new("/v1").mount(kynos::routes![
        revoke_session,
        create_session,
        status
    ]));
}
