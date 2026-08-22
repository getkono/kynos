//! A JWT verifier the framework does not ship.
//!
//! ```text
//! cargo run -p kynos --example jwt --no-default-features \
//!   --features openapi31,macros,server,http1,json
//! ```
//!
//! Kynos deliberately has no `jwt` feature. Which algorithm, which claims,
//! which issuer, how keys rotate and what a subject *means* are all decisions
//! an application has already made by the time it reaches for a framework, and
//! a framework that made them for you would be wrong for most callers and
//! unremovable for the rest. `jsonwebtoken` is a dev-dependency of this file
//! and is named nowhere under `src/`.
//!
//! What Kynos does supply is everything around the token, which is the part
//! that is the same for everyone and the part that is easy to get wrong.
//!
//! Seven things are worth noticing:
//!
//! * **Nothing here finds the token.** `#[security(bearer(format = "JWT"))]`
//!   already said where a bearer token travels, so the carrier is emitted from
//!   that same attribute and `authenticate` receives a [`BearerToken`]. There
//!   is no `strip_prefix("Bearer ")` to write, and no way to accidentally read
//!   a field the description does not advertise. RFC 9110 makes the scheme name
//!   case-insensitive and RFC 6750 permits extra whitespace after it; a
//!   hand-rolled finder usually honours neither.
//! * **`bearerFormat` is documentation, not parsing.** A bearer token is opaque
//!   by definition. `format = "JWT"` tells a human reading the description what
//!   to expect; Kynos never looks inside.
//! * **Key rotation is a `kid` lookup, and it belongs here.** The header names
//!   the key, `Keys` resolves it, and an unknown `kid` fails exactly like a bad
//!   signature. A framework holding this would have to hold your key store too.
//! * **Every failure is `unauthenticated`.** Expired, wrong issuer, unknown
//!   `kid`, bad signature — telling a caller which one it was tells an attacker
//!   which tokens exist and which keys are live.
//! * **Scopes are a type, and they are checked where they are declared.**
//!   `Scoped<AccessToken, ReadReports>` writes `reports:read` into the
//!   operation's security requirement *and* into the check. A misspelling is a
//!   compile error rather than a silent grant.
//! * **`MaybeAuth` is a description, not a flag.** `/feed` declares
//!   `[{}, {AccessToken: []}]`, so a reader learns the credential is honoured
//!   rather than demanded. A token that is present and *wrong* is still a 401
//!   there: only absence is anonymity.
//! * **Issuing a token is an ordinary operation.** `POST /session` is guarded
//!   by `Auth<Basic<Credentials>>`, so both carriers are exercised, and the
//!   password is compared with [`constant_time_eq`] because comparing a shared
//!   secret with `==` leaks how much of it was right.
//!
//! [`BearerToken`]: kynos::security::carrier::BearerToken
//! [`constant_time_eq`]: kynos::security::constant_time_eq

use std::{
    collections::HashMap,
    net::Ipv4Addr,
    time::{SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use kynos::{
    error::rejection::AuthRejection,
    prelude::*,
    security::{
        Authenticates, Authenticator,
        auth::{Auth, MaybeAuth, Scoped, Scopes},
        carrier::BearerToken,
        constant_time_eq,
        schemes::{Basic, Credentials},
    },
    server::Server,
};
use serde::{Deserialize, Serialize};

/// Who this service issues tokens as, and who it will not.
const ISSUER: &str = "https://auth.example.com";

/// How long a token is good for.
const TOKEN_LIFETIME: u64 = 900;

/// The audience a token has to name to be one of ours.
///
/// Without it a token minted for a *different* service by the same issuer would
/// authenticate here — the confused-deputy shape that makes `aud` mandatory in
/// practice rather than optional.
const AUDIENCE: &str = "https://api.example.com";

// --- What a token says ----------------------------------------------------

/// The claims this service mints and reads.
///
/// A type rather than a map, so a claim that is not written down is a claim no
/// handler can read.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Claims {
    /// The subject: who the token is about.
    sub: String,
    /// The issuer, checked rather than trusted.
    iss: String,
    /// The audience, likewise.
    aud: String,
    /// Expiry, as seconds since the epoch.
    exp: u64,
    /// Not-before, so a token minted for later cannot be used now.
    nbf: u64,
    /// The granted scopes, space-delimited per RFC 6749 section 3.3.
    scope: String,
}

impl Claims {
    /// The scopes this token grants.
    fn scopes(&self) -> impl Iterator<Item = &str> {
        self.scope.split(' ').filter(|scope| !scope.is_empty())
    }
}

/// The token a successful sign-in returns, in RFC 6749 section 5.1's shape.
///
/// The field names are the specification's, not this file's: `access_token` and
/// `token_type` are what an OAuth 2.0 client reads, so renaming them to please a
/// lint would produce a response no client understands.
#[derive(Schema, Serialize)]
#[expect(
    clippy::struct_field_names,
    reason = "RFC 6749 section 5.1 fixes these names"
)]
struct Token {
    access_token: String,
    token_type: String,
    expires_in: u64,
}

// --- The schemes ----------------------------------------------------------

/// A short-lived access token.
///
/// `format = "JWT"` is a hint for a human reading the description. The wire
/// form of a bearer token is opaque by definition, and a description claiming
/// to know it would be claiming more than it can check.
#[derive(SecurityScheme)]
#[security(bearer(format = "JWT"))]
#[security(credential = Claims, description = "A short-lived access token")]
struct AccessToken;

/// The scopes `/reports` demands.
///
/// A type rather than a string literal, so a misspelling is a compile error and
/// renaming one is a single edit.
struct ReadReports;

impl Scopes for ReadReports {
    const SCOPES: &'static [&'static str] = &["reports:read"];
}

// --- Verifying ------------------------------------------------------------

/// One signing key, and the identifier a token names it by.
struct Key {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

/// The keys this service will accept a token from.
///
/// Two of them, because rotation is the case a single key hides: a new key
/// starts signing while the previous one keeps verifying until every token it
/// signed has expired. A framework that owned this would have to own the store
/// the keys come from.
struct Keys {
    /// The key new tokens are signed with.
    current: String,
    by_id: HashMap<String, Key>,
}

impl Keys {
    /// Two keys, seeded. A real service reads these from a secret manager.
    fn seeded() -> Self {
        let mut by_id = HashMap::new();
        for (id, secret) in [
            ("k1", &b"the-previous-secret"[..]),
            ("k2", &b"the-current-secret"[..]),
        ] {
            by_id.insert(
                id.to_owned(),
                Key {
                    encoding: EncodingKey::from_secret(secret),
                    decoding: DecodingKey::from_secret(secret),
                },
            );
        }
        Self {
            current: "k2".to_owned(),
            by_id,
        }
    }

    /// Mints a token for `subject`, granting `scopes`.
    ///
    /// Infallible, and the two `expect`s say why: the current key is one this
    /// store seeded, and HMAC signing of a serializable claim set has no
    /// failure mode. A real store reading from a secret manager would have one,
    /// and would return a `Result` that an `ApiError` turns into a declared 500.
    fn issue(&self, subject: &str, scopes: &str) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_secs();

        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(self.current.clone());

        let claims = Claims {
            sub: subject.to_owned(),
            iss: ISSUER.to_owned(),
            aud: AUDIENCE.to_owned(),
            exp: now + TOKEN_LIFETIME,
            nbf: now,
            scope: scopes.to_owned(),
        };

        let key = self
            .by_id
            .get(&self.current)
            .expect("the current key is one this store holds");
        encode(&header, &claims, &key.encoding).expect("HMAC signing does not fail")
    }
}

impl<C: Sync> Authenticator<AccessToken, C> for Keys {
    async fn authenticate(
        &self,
        presented: BearerToken,
        context: &C,
    ) -> Result<Claims, AuthRejection> {
        let _ = context;

        // The `kid` says which key signed this, which is the whole of rotation.
        // Reading the header is not trusting it: nothing below the signature
        // check is believed, and an unknown `kid` fails exactly like a forgery.
        let header =
            jsonwebtoken::decode_header(presented.as_str()).map_err(unauthenticated_whatever)?;
        let key = header
            .kid
            .and_then(|kid| self.by_id.get(&kid))
            .ok_or_else(AuthRejection::unauthenticated)?;

        let mut validation = Validation::new(Algorithm::HS256);
        // The algorithm is fixed here rather than read from the header. A
        // verifier that accepted whatever the token asked for is the `alg=none`
        // family of forgeries.
        validation.set_issuer(&[ISSUER]);
        validation.set_audience(&[AUDIENCE]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        // A small allowance for clocks that disagree, which they do.
        validation.leeway = 30;

        decode::<Claims>(presented.as_str(), &key.decoding, &validation)
            .map(|token| token.claims)
            .map_err(unauthenticated_whatever)
    }

    async fn authorize(
        &self,
        credential: &Claims,
        scopes: &'static [&'static str],
        context: &C,
    ) -> Result<(), AuthRejection> {
        let _ = context;

        // Every demanded scope must be granted. `Forbidden` rather than
        // `Unauthenticated`: the token was valid, it simply does not reach.
        if scopes
            .iter()
            .all(|demanded| credential.scopes().any(|held| held == *demanded))
        {
            Ok(())
        } else {
            Err(AuthRejection::Forbidden)
        }
    }
}

/// Every verification failure is the same 401.
///
/// Expired, wrong issuer, wrong audience, bad signature: telling a caller which
/// one it was tells an attacker which tokens exist and which keys are live.
fn unauthenticated_whatever<E>(error: E) -> AuthRejection {
    let _ = error;
    AuthRejection::unauthenticated()
}

// --- Signing in -----------------------------------------------------------

/// The one place a password is checked.
struct Passwords;

impl<C: Sync> Authenticator<Basic<Credentials>, C> for Passwords {
    async fn authenticate(
        &self,
        presented: Credentials,
        context: &C,
    ) -> Result<Credentials, AuthRejection> {
        let _ = context;

        // A real service compares against a password *hash* — argon2 or scrypt
        // — and Kynos ships neither, for the reason it ships no JWT verifier.
        // What it does ship is the comparison: `==` on a shared secret returns
        // at the first byte that differs, so how long it took says how much of
        // the guess was right.
        let known = presented.username == "reporter" || presented.username == "reader";
        let correct = constant_time_eq(presented.password.as_bytes(), b"correct-horse");

        // Both checks always run, so an unknown user and a wrong password take
        // the same path and the same time.
        if known && correct {
            Ok(presented)
        } else {
            Err(AuthRejection::unauthenticated())
        }
    }

    async fn authorize(
        &self,
        _: &Credentials,
        _: &'static [&'static str],
        _: &C,
    ) -> Result<(), AuthRejection> {
        Ok(())
    }
}

// --- The application context ----------------------------------------------

/// One context, two schemes.
///
/// A router guarding a scheme this context cannot verify does not compile —
/// which is a different guarantee from a middleware that is simply absent at
/// run time.
struct App {
    keys: std::sync::Arc<Keys>,
    passwords: Passwords,
}

/// The key store reaches `sign_in` as a dependency and the authenticator as a
/// borrow, which is why it is behind an `Arc`: `Provides` hands out an owned
/// value, and one store is one store.
impl kynos::di::Provides<std::sync::Arc<Keys>> for App {
    fn provide(&self) -> std::sync::Arc<Keys> {
        std::sync::Arc::clone(&self.keys)
    }
}

impl Authenticates<AccessToken> for App {
    type Authenticator = Keys;

    fn authenticator(&self) -> &Self::Authenticator {
        &self.keys
    }
}

impl Authenticates<Basic<Credentials>> for App {
    type Authenticator = Passwords;

    fn authenticator(&self) -> &Self::Authenticator {
        &self.passwords
    }
}

// --- The operations -------------------------------------------------------

/// Exchanges a password for a token.
///
/// Guarded by `Auth<Basic<..>>`, so the operation that *issues* a credential is
/// itself described as requiring one — which is what stops a token endpoint
/// quietly becoming the unauthenticated hole in an otherwise guarded API.
#[kynos::post("/session")]
async fn sign_in(
    Auth(credentials): Auth<Basic<Credentials>>,
    Inject(keys): Inject<std::sync::Arc<Keys>>,
) -> Json<Token> {
    // The scopes a caller gets are the service's decision, not the caller's.
    let scopes = if credentials.username == "reporter" {
        "reports:read"
    } else {
        ""
    };

    Json(Token {
        access_token: keys.issue(&credentials.username, scopes),
        token_type: "Bearer".to_owned(),
        expires_in: TOKEN_LIFETIME,
    })
}

/// Reads the caller's own subject.
#[kynos::get("/me")]
async fn me(Auth(claims): Auth<AccessToken>) -> Json<Subject> {
    Json(Subject {
        subject: claims.sub,
    })
}

/// What `/me` returns.
#[derive(Schema, Serialize)]
struct Subject {
    subject: String,
}

/// Reads reports, which needs a scope the token has to carry.
#[kynos::get("/reports")]
async fn reports(caller: Scoped<AccessToken, ReadReports>) -> NoContent {
    let _ = caller.into_inner();
    NoContent
}

/// A feed that is richer when the caller identified themselves.
#[kynos::get("/feed")]
async fn feed(caller: MaybeAuth<AccessToken>) -> Json<Subject> {
    Json(Subject {
        subject: caller
            .into_inner()
            .map_or_else(|| "anonymous".to_owned(), |claims| claims.sub),
    })
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<App>::new().mount(kynos::routes![sign_in, me, reports, feed]);

    println!("{}", router.openapi()?.to_json()?);

    let context = App {
        keys: std::sync::Arc::new(Keys::seeded()),
        passwords: Passwords,
    };

    Server::new(router.build(context)?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
