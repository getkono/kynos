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
//! * **A context declares what it can verify.** One `App`, seven
//!   implementations, so the set of schemes an application supports is visible
//!   in one place rather than spread across the handlers requiring them. Only
//!   one of them verifies for real; the other six say so in their name.
//!
//! The challenge is declared on the scheme rather than on the authenticator, so
//! the `WWW-Authenticate` a client receives and the one the description
//! advertises are one string.

use std::{collections::HashMap, net::Ipv4Addr};

use kynos::{
    error::rejection::AuthRejection,
    prelude::*,
    security::{
        Authenticates, Authenticator,
        auth::{Auth, MaybeAuth, Scoped, Scopes},
        carrier::{BearerToken, Carries},
        schemes::{Basic, Credentials, MutualTls},
    },
    server::Server,
};

/// What a verified token yields a handler.
///
/// `scopes` is what makes `authorize` a real check rather than a formality: the
/// grants ride with the credential, so the comparison is against something the
/// token actually said.
#[derive(Clone, Debug)]
struct Claims {
    subject: String,
    scopes: Vec<String>,
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
///
/// A flow's `scopes` takes either spelling. `"users:read" = "..."` gives the
/// scope the description an authorization server shows on its consent screen;
/// a bare `"users:write"` names one with none. The scheme-level `scopes(..)`
/// below is a different thing again — what this scheme demands by default
/// rather than what the server publishes.
#[derive(SecurityScheme)]
#[security(oauth2(authorization_code(
    authorization_url = "https://auth.example.com/authorize",
    token_url = "https://auth.example.com/token",
    refresh_url = "https://auth.example.com/token",
    scopes("users:read" = "Read a user's profile", "users:write"),
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

/// A verifier that actually verifies.
///
/// Opaque tokens against a table, because the point is the *shape* of the two
/// methods rather than the cryptography: a real one checks a signature or hits
/// a session store, and Kynos ships neither. What matters is that
/// `authenticate` reads the request and can fail, and that `authorize` compares
/// the scopes an operation demands against the ones the credential carries.
struct Tokens {
    issued: HashMap<&'static str, Claims>,
}

impl Tokens {
    /// Two callers, one of whom may read reports.
    fn seeded() -> Self {
        let mut issued = HashMap::new();
        issued.insert(
            "tok_reader",
            Claims {
                subject: "user-1".to_owned(),
                scopes: vec!["reports:read".to_owned()],
            },
        );
        issued.insert(
            "tok_plain",
            Claims {
                subject: "user-2".to_owned(),
                scopes: Vec::new(),
            },
        );
        Self { issued }
    }
}

impl<C: Sync> Authenticator<AccessToken, C> for Tokens {
    async fn authenticate(
        &self,
        presented: BearerToken,
        context: &C,
    ) -> Result<Claims, AuthRejection> {
        let _ = context;

        // What this does *not* do is find the token. `#[security(bearer)]`
        // already said where it travels, so RFC 6750's framing -- the
        // `Authorization` field, the case-insensitive scheme name, the space --
        // is read by the carrier the same attribute wrote. All that is left
        // here is what the token means.
        self.issued
            .get(presented.as_str())
            .cloned()
            .ok_or_else(AuthRejection::unauthenticated)
    }

    async fn authorize(
        &self,
        credential: &Claims,
        scopes: &'static [&'static str],
        context: &C,
    ) -> Result<(), AuthRejection> {
        let _ = context;

        // Every demanded scope must be granted. `Forbidden` and not
        // `Unauthenticated`: the credential was valid, it just does not reach.
        if scopes
            .iter()
            .all(|demanded| credential.scopes.iter().any(|held| held == demanded))
        {
            Ok(())
        } else {
            Err(AuthRejection::Forbidden)
        }
    }
}

/// One stand-in, for the six schemes whose verifier would be arbitrary.
///
/// A session is checked against a store, a client certificate against a CA, a
/// federated identity against a discovery document — none of that is Kynos's
/// business, and inventing six of them would teach nothing `Tokens` does not.
/// What this one still shows is the shape: an authenticator is chosen by the
/// compiler from the scheme type, so a handler taking `Auth<S>` against a
/// context that cannot verify `S` is a compile error rather than a 500.
struct Rejects;

impl<S: Carries, C: Sync> Authenticator<S, C> for Rejects
where
    // `Sync` and not `Default`: nothing here builds a credential, but
    // `authorize` holds a reference to one across an await, and the trait
    // requires that future to be `Send`.
    S::Credential: Sync,
{
    async fn authenticate(
        &self,
        presented: S::Presented,
        context: &C,
    ) -> Result<S::Credential, AuthRejection> {
        let _ = (presented, context);
        Err(AuthRejection::unauthenticated())
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
/// Two fields, seven implementations. A context declares what it can verify, so
/// the set of schemes an application supports is visible in one place rather
/// than spread across the handlers that happen to require them.
struct App {
    tokens: Tokens,
    verifier: Rejects,
}

/// The one scheme with a real verifier, wired by hand.
///
/// The association is per scheme, so `AccessToken` reaching `Tokens` while
/// everything else reaches `Rejects` costs one implementation rather than a
/// branch anywhere.
impl Authenticates<AccessToken> for App {
    type Authenticator = Tokens;

    fn authenticator(&self) -> &Self::Authenticator {
        &self.tokens
    }
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

/// Serves a feed, personalised when the caller identified themselves.
///
/// `MaybeAuth` declares `security: [{}, {Bearer: []}]` — the empty requirement
/// first, which is OpenAPI's spelling for "anonymous access is also permitted".
/// A reader learns that the credential is *honoured* rather than *demanded*,
/// which no flag on a middleware can say.
///
/// A token that is present and wrong is still a 401. Only absence is anonymity:
/// a client that sent a broken credential is not an anonymous client, and
/// treating it as one would wave through the request most worth refusing.
#[kynos::get("/feed")]
async fn get_feed(caller: MaybeAuth<AccessToken>) -> NoContent {
    if let Some(claims) = caller.into_inner() {
        let _ = claims.subject;
    }
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
            get_feed,
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

    let context = App {
        tokens: Tokens::seeded(),
        verifier: Rejects,
    };

    Server::new(router.build(context)?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
