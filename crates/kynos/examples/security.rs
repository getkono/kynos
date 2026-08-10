//! Requiring a credential and describing it are one act.
//!
//! ```text
//! cargo run -p kynos --example security
//! ```
//!
//! A `CurrentUser` read from an `Authorization` header is not application
//! state, and injecting it would make the requirement invisible in the
//! description. It arrives through `Auth<S>` instead, which is both the
//! enforcement and the declaration: an operation that takes one cannot be
//! served without the credential, and cannot be described without saying so.
//!
//! The context proves it can authenticate. A router built with a context that
//! implements no `Authenticates<Bearer<Claims>>` does not compile, in the same
//! way and at the same place as a missing dependency.

use std::net::Ipv4Addr;

use kynos::{
    error::rejection::AuthRejection,
    http::Parts,
    prelude::*,
    response::status::NoContent,
    security::{
        Authenticates, Authenticator,
        auth::{Auth, Scoped, Scopes},
        schemes::Bearer,
    },
    server::Server,
};

/// What a verified token yields the handler.
///
/// The scheme is generic over this precisely so an application is not stuck
/// handing handlers a raw token: the *description* says "a bearer token"
/// either way, and what the token means is the application's business.
#[derive(Clone, Debug)]
struct Claims {
    subject: String,
}

/// Verifies tokens.
struct Tokens {
    _key: &'static str,
}

impl<C: Sync> Authenticator<Bearer<Claims>, C> for Tokens {
    async fn authenticate(&self, parts: &Parts, context: &C) -> Result<Claims, AuthRejection> {
        let _ = (parts, context);
        Err(AuthRejection::Unauthenticated)
    }

    async fn authorize(
        &self,
        credential: &Claims,
        scopes: &'static [&'static str],
        context: &C,
    ) -> Result<(), AuthRejection> {
        let _ = (credential, scopes, context);
        Err(AuthRejection::Forbidden)
    }
}

/// The application context.
struct App {
    tokens: Tokens,
}

/// The context proving it can authenticate this scheme.
///
/// One authenticator per scheme type, chosen by the compiler rather than looked
/// up — which is why a handler taking `Auth<Bearer<Claims>>` against a context
/// without this implementation is a compile error rather than a 500.
impl Authenticates<Bearer<Claims>> for App {
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Self::Authenticator {
        &self.tokens
    }
}

/// The scopes an operation demands, as a type.
struct ReadReports;

impl Scopes for ReadReports {
    const SCOPES: &'static [&'static str] = &["reports:read"];
}

/// Any verified caller may see this.
#[kynos::get("/me")]
async fn me(Auth(claims): Auth<Bearer<Claims>>) -> NoContent {
    let _ = claims.subject;
    NoContent
}

/// Only a caller holding `reports:read` may see this.
///
/// The scopes are part of the type, so the description and the check cannot
/// name different ones.
#[kynos::get("/reports")]
async fn reports(caller: Scoped<Bearer<Claims>, ReadReports>) -> NoContent {
    let _ = caller.into_inner();
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let context = App {
        tokens: Tokens { _key: "secret" },
    };

    let service = Router::<App>::new()
        .security_scheme::<Bearer<Claims>>()
        .mount(kynos::routes![me, reports])
        .build(context)?;

    Server::new(service)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
