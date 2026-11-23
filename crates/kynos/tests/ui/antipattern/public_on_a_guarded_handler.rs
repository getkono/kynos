//! An operation that takes a guard cannot also declare that it takes none.
//!
//! `#[kynos::public]` answers a group's question about a handler without a
//! guard; on a guarded one it could only mislead a reader. `MaybeAuth` is how
//! a guarded operation admits anonymous callers. Its pass sibling,
//! `ui/pass/public_beside_a_route_attribute.rs`, marks only handlers taking no
//! guard.

use kynos::{
    Router,
    error::rejection::AuthRejection,
    response::status::NoContent,
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
#[kynos::public]
async fn revoke_session(caller: Auth<Bearer>) -> NoContent {
    let _ = caller;
    NoContent
}

fn main() {
    let _ = Router::<App>::new().mount(kynos::routes![revoke_session]);
}
