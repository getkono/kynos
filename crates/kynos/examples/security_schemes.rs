//! Every kind of security scheme, declared as a type.
//!
//! Run it without the JSON codec, since nothing here has a body:
//!
//! ```text
//! cargo run -p kynos --example security_schemes --no-default-features \
//!   --features openapi31,macros,server,http1
//! ```
//!
//! A credential is required and described by the same act. `Auth<S>` is not
//! application state and injecting it would make the requirement invisible: an
//! operation taking one cannot be served without the credential, and cannot be
//! described without saying so. The context proves it can authenticate, so a
//! router built with a context implementing no `Authenticates<S>` does not
//! compile — in the same way, and at the same place, as a missing dependency.
//!
//! Five things are worth noticing:
//!
//! * **The scheme's kind is in the attribute, not in a string somewhere.** A
//!   misspelled kind is a compile error, and the grammar nests the kind so that
//!   `name` is unambiguous — the component key a document registers the scheme
//!   under and the header an API key travels in are different things that would
//!   otherwise share a word.
//! * **`Credential` is the application's type.** The description says "a bearer
//!   token" whatever the handler receives, so there is no reason to force a raw
//!   string on it. Verifying is a separate trait for the same reason: Kynos
//!   ships no JWT verifier and no session store, because that is policy.
//! * **`Authorization` cannot be an API key.** The derive rejects it, along
//!   with `Accept` and `Content-Type`, because the specification says a
//!   parameter definition for those is ignored. An API key in a header is for
//!   the `X-Api-Key`-shaped schemes; a credential in `Authorization` is `http`,
//!   `bearer` or `basic`.
//! * **Scopes are a type either way, and the two ways differ.** `DelegatedAccess`
//!   declares them on the *scheme*, which is what OAuth 2.0 publishes as the set
//!   an authorization server can grant. `ReadReports` declares them on the
//!   *operation* through `Scoped`, which is what this particular endpoint
//!   demands. Both are checked, neither is a string a handler passes.
//! * **A context declares what it can verify.** One `App`, eight
//!   implementations, so the set of schemes an application supports is visible
//!   in one place rather than spread across the handlers requiring them.
//!
//! The challenge is declared on the scheme rather than on the authenticator, so
//! the `WWW-Authenticate` a client receives and the one the description
//! advertises are one string.

use std::net::Ipv4Addr;

use kynos::{
    error::rejection::AuthRejection,
    http::Parts,
    prelude::*,
    security::{
        Authenticates, Authenticator, SecurityScheme,
        auth::{Auth, Scoped, Scopes},
        schemes::{Basic, Credentials, MutualTls},
    },
    server::Server,
};

/// What a verified token yields a handler.
#[derive(Clone, Debug)]
struct Claims {
    subject: String,
}

/// A bearer token, in the `Authorization` header.
///
/// `format` is documentation for a human rather than something Kynos parses:
/// the wire form of a bearer token is opaque by definition, and a description
/// that claimed to know it would be claiming more than it can check.
#[derive(SecurityScheme)]
#[security(bearer(format = "JWT"))]
#[security(credential = Claims, description = "A short-lived access token")]
struct AccessToken;

/// A machine key, in a header of this service's choosing.
///
/// `in` and `name` are both required: a key that does not say where it travels
/// is a scheme a client cannot use, and there is no default worth guessing.
#[derive(SecurityScheme)]
#[security(api_key(in = "header", name = "X-Api-Key"))]
#[security(name = "ServiceKey", description = "Issued per integration")]
struct ServiceKey;

/// A session, in a cookie.
///
/// The same scheme kind as above with a different location, which is why the
/// location is a member rather than three separate kinds.
#[derive(SecurityScheme)]
#[security(api_key(in = "cookie", name = "session"))]
struct SessionCookie;

/// Username and password, over the `Authorization` header.
///
/// `Credentials` is the shape Kynos supplies for the decoded pair, because
/// unlike a bearer token, basic authentication *does* have a wire form the
/// specification fixes.
#[derive(SecurityScheme)]
#[security(basic)]
#[security(credential = Credentials, challenge = "Basic realm=\"admin\", charset=\"UTF-8\"")]
struct AdminLogin;

/// A client certificate, presented during the TLS handshake.
///
/// The one scheme carried by no request field at all, which is exactly why it
/// has to be declared: nothing else in the request would reveal it.
#[derive(SecurityScheme)]
#[security(mutual_tls)]
#[security(credential = Vec<u8>, description = "A certificate issued by the partner CA")]
struct PartnerCertificate;

/// Delegated authorization.
///
/// The scopes are part of the scheme rather than of the handler, so an
/// operation requiring them and a document advertising them cannot disagree.
#[derive(SecurityScheme)]
#[security(oauth2(authorization_code(
    authorization_url = "https://auth.example.com/authorize",
    token_url = "https://auth.example.com/token",
    refresh_url = "https://auth.example.com/token",
    scopes("users:read", "users:write"),
),))]
#[security(name = "DelegatedAccess", scopes("users:read"))]
struct DelegatedAccess;

/// An identity provider that publishes its own metadata.
///
/// One URL instead of every endpoint, because discovery is what OpenID Connect
/// adds over bare OAuth 2.0.
#[derive(SecurityScheme)]
#[security(openid_connect(url = "https://auth.example.com/.well-known/openid-configuration"))]
struct Federated;

/// One verifier, standing in for seven.
///
/// Real ones differ — a token is verified against a key, a session against a
/// store — and none of that is Kynos's business. What matters here is the
/// shape: an authenticator is chosen by the compiler from the scheme type, so
/// a handler taking `Auth<S>` against a context that cannot verify `S` is a
/// compile error rather than a 500.
struct Rejects;

impl<S: SecurityScheme, C: Sync> Authenticator<S, C> for Rejects
where
    // `Sync` and not `Default`: nothing here builds a credential, but
    // `authorize` holds a reference to one across an await, and the trait
    // requires that future to be `Send`.
    S::Credential: Sync,
{
    async fn authenticate(
        &self,
        parts: &Parts,
        context: &C,
    ) -> Result<S::Credential, AuthRejection> {
        let _ = (parts, context);
        Err(AuthRejection::Unauthenticated)
    }

    async fn authorize(
        &self,
        credential: &S::Credential,
        scopes: &'static [&'static str],
        context: &C,
    ) -> Result<(), AuthRejection> {
        let _ = (credential, scopes, context);
        Err(AuthRejection::Forbidden)
    }
}

/// The application context.
///
/// One field, seven implementations. A context declares what it can verify, and
/// the set of schemes an application supports is therefore visible in one place
/// rather than spread across the handlers that happen to require them.
struct App {
    verifier: Rejects,
}

macro_rules! verifies {
    ($($scheme:ty),+ $(,)?) => {
        $(
            impl Authenticates<$scheme> for App {
                type Authenticator = Rejects;

                fn authenticator(&self) -> &Self::Authenticator {
                    &self.verifier
                }
            }
        )+
    };
}

verifies!(
    AccessToken,
    ServiceKey,
    SessionCookie,
    Basic<Credentials>,
    MutualTls,
    DelegatedAccess,
    Federated,
);

/// Reads the caller's own profile.
///
/// `Auth<S>` is both the enforcement and the declaration: this cannot be served
/// without the credential, and cannot be described without saying so.
#[kynos::get("/me")]
async fn get_me(auth: Auth<AccessToken>) -> NoContent {
    let _ = auth.into_inner().subject;
    NoContent
}

/// Reports usage for an integration.
#[kynos::get("/usage")]
async fn get_usage(auth: Auth<ServiceKey>) -> NoContent {
    let _ = auth.into_inner();
    NoContent
}

/// Reads a session-backed preference.
#[kynos::get("/preferences")]
async fn get_preferences(auth: Auth<SessionCookie>) -> NoContent {
    let _ = auth.into_inner();
    NoContent
}

/// Signs an administrator in.
#[kynos::post("/admin/session")]
async fn admin_sign_in(auth: Auth<Basic<Credentials>>) -> NoContent {
    let _ = auth.into_inner();
    NoContent
}

/// Accepts a partner's mutually authenticated call.
#[kynos::post("/partners/events")]
async fn partner_event(auth: Auth<MutualTls>) -> NoContent {
    let _ = auth.into_inner();
    NoContent
}

/// Lists users on a resource owner's behalf.
#[kynos::get("/delegated/users")]
async fn delegated_users(auth: Auth<DelegatedAccess>) -> NoContent {
    let _ = auth.into_inner();
    NoContent
}

/// Reads a federated identity.
#[kynos::get("/federated/me")]
async fn federated_me(auth: Auth<Federated>) -> NoContent {
    let _ = auth.into_inner();
    NoContent
}

/// The scopes one operation demands, as a type.
///
/// Distinct from the scopes on `DelegatedAccess`: those are what the
/// authorization server publishes it can grant, these are what this endpoint
/// insists on. A scheme can offer more than any one operation needs.
struct ReadReports;

impl Scopes for ReadReports {
    const SCOPES: &'static [&'static str] = &["reports:read"];
}

/// Only a caller holding `reports:read` may see this.
///
/// `Scoped` rather than `Auth`, so the scopes are part of the argument's type.
/// The description and the check read them from the same place and cannot name
/// different ones.
#[kynos::get("/reports")]
async fn reports(caller: Scoped<AccessToken, ReadReports>) -> NoContent {
    let _ = caller.into_inner();
    NoContent
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<App>::new()
        // A scheme reaches `components.securitySchemes` by being declared here,
        // whether or not an operation names it — which is what lets a scheme be
        // published before the operations that will require it exist.
        .security_scheme::<AdminLogin>()
        .security_scheme::<PartnerCertificate>()
        .mount(kynos::routes![
            get_me,
            get_usage,
            get_preferences,
            admin_sign_in,
            partner_event,
            delegated_users,
            federated_me,
            reports,
        ]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(App { verifier: Rejects })?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
