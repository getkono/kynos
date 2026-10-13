//! The extraction group's handlers: each answers what its base answers, read
//! through one more extractor, or one extractor read another way.
//!
//! The GET rows return `plaintext`'s `Hello, World!` and the body rows
//! `echo-post`'s document, so the response written is the base's.

use kynos::{
    error::rejection::AuthRejection,
    extract::body::{OneOf, form::Form, text::Text},
    extract::params::querystring::QueryString,
    http::media,
    prelude::*,
    security::{
        Authenticates, Authenticator,
        carrier::{ApiKey, BearerToken, Credentials},
        constant_time_eq,
        schemes::{Basic, Bearer},
    },
};
use serde::Deserialize;

use super::{App, Echo, Item, Page};

/// What `inject` reads from the context.
#[derive(Clone, Copy)]
pub(crate) struct Greeting(pub(crate) &'static str);

/// The one caller every guard admits.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Caller;

/// The credentials the guard rows present, and the ones their authenticator
/// holds them to.
pub(crate) const TOKEN: &str = "Bearer kynos-profile-token";
/// `auth-401`'s token: well-formed, and not the one the authenticator holds.
pub(crate) const WRONG_TOKEN: &str = "Bearer kynos-profile-tokeN";
/// `ada:lovelace`, as RFC 7617 carries it.
pub(crate) const BASIC: &str = "Basic YWRhOmxvdmVsYWNl";
pub(crate) const API_KEY: &str = "kynos-profile-key";

const SECRET: &[u8] = b"kynos-profile-token";

/// A header-carried API key.
#[derive(SecurityScheme)]
#[security(api_key(in = "header", name = "X-Api-Key"))]
#[security(credential = Caller)]
pub(crate) struct ServiceKey;

/// The authenticator every guard row reaches: one comparison in constant time
/// against a credential it holds, as an application checking an opaque token
/// against its own would do before anything else.
pub(crate) struct Keys;

impl Authenticator<Bearer<Caller>, App> for Keys {
    async fn authenticate(
        &self,
        presented: BearerToken,
        context: &App,
    ) -> Result<Caller, AuthRejection> {
        let _ = context;
        constant_time_eq(presented.as_str().as_bytes(), SECRET)
            .then_some(Caller)
            .ok_or_else(AuthRejection::unauthenticated)
    }

    async fn authorize(
        &self,
        credential: &Caller,
        scopes: &[kynos::security::Unscoped],
        context: &App,
    ) -> Result<(), AuthRejection> {
        let _ = (credential, scopes, context);
        Ok(())
    }
}

impl Authenticator<Basic<Caller>, App> for Keys {
    async fn authenticate(
        &self,
        presented: Credentials,
        context: &App,
    ) -> Result<Caller, AuthRejection> {
        let _ = context;
        (constant_time_eq(presented.username().as_bytes(), b"ada")
            & constant_time_eq(presented.password().as_bytes(), b"lovelace"))
        .then_some(Caller)
        .ok_or_else(AuthRejection::unauthenticated)
    }

    async fn authorize(
        &self,
        credential: &Caller,
        scopes: &[kynos::security::Unscoped],
        context: &App,
    ) -> Result<(), AuthRejection> {
        let _ = (credential, scopes, context);
        Ok(())
    }
}

impl Authenticator<ServiceKey, App> for Keys {
    async fn authenticate(
        &self,
        presented: ApiKey,
        context: &App,
    ) -> Result<Caller, AuthRejection> {
        let _ = context;
        constant_time_eq(presented.as_str().as_bytes(), API_KEY.as_bytes())
            .then_some(Caller)
            .ok_or_else(AuthRejection::unauthenticated)
    }

    async fn authorize(
        &self,
        credential: &Caller,
        scopes: &[kynos::security::Unscoped],
        context: &App,
    ) -> Result<(), AuthRejection> {
        let _ = (credential, scopes, context);
        Ok(())
    }
}

impl Authenticates<Bearer<Caller>> for App {
    type Authenticator = Keys;

    fn authenticator(&self) -> &Keys {
        &self.keys
    }
}

impl Authenticates<Basic<Caller>> for App {
    type Authenticator = Keys;

    fn authenticator(&self) -> &Keys {
        &self.keys
    }
}

impl Authenticates<ServiceKey> for App {
    type Authenticator = Keys;

    fn authenticator(&self) -> &Keys {
        &self.keys
    }
}

/// `plaintext`'s answer, with one `Inject` read.
#[kynos::get("/inject")]
async fn inject(Inject(greeting): Inject<Greeting>) -> Text {
    Text(greeting.0.to_owned())
}

/// `plaintext`'s answer, behind a bearer guard.
#[kynos::get("/auth/bearer")]
async fn auth_bearer(caller: Auth<Bearer<Caller>>) -> Text {
    let _ = caller.into_inner();
    Text("Hello, World!".to_owned())
}

/// `plaintext`'s answer, behind a basic guard.
#[kynos::get("/auth/basic")]
async fn auth_basic(caller: Auth<Basic<Caller>>) -> Text {
    let _ = caller.into_inner();
    Text("Hello, World!".to_owned())
}

/// `plaintext`'s answer, behind an API-key guard.
#[kynos::get("/auth/api-key")]
async fn auth_api_key(caller: Auth<ServiceKey>) -> Text {
    let _ = caller.into_inner();
    Text("Hello, World!".to_owned())
}

/// One declared cookie.
#[derive(kynos::CookieParams)]
struct Session {
    session: String,
}

/// `plaintext`'s answer, with one declared cookie read.
#[kynos::get("/cookies")]
async fn cookies(Cookies(session): Cookies<Session>) -> Text {
    let _ = session.session;
    Text("Hello, World!".to_owned())
}

/// `path-params`' three captures, each held to the bounds its schema declares.
#[derive(Schema, PathParams)]
struct Bounded {
    #[schema(minimum = 1, maximum = 1_000)]
    a: u64,
    #[schema(minimum = 1, maximum = 1_000)]
    b: u64,
    #[schema(minimum = 1, maximum = 1_000)]
    c: u64,
}

/// `path-params`, its captures bounded.
#[kynos::get("/bounded/{a}/{b}/{c}")]
async fn bounded(Path(item): Path<Bounded>, Query(page): Query<Page>) -> Json<u64> {
    Json(item.a + item.b + item.c + page.after.unwrap_or(0) + u64::from(page.per))
}

/// `path-params`' query parameters, as the one JSON document OpenAPI 3.2's
/// `in: querystring` describes.
#[derive(Schema, Deserialize)]
struct PageDocument {
    after: Option<u64>,
    per_page: u32,
}

/// `path-params`, its query read whole.
#[kynos::get("/querystring/{a}/{b}/{c}")]
async fn querystring(
    Path(item): Path<Item>,
    query: QueryString<PageDocument, media::Json>,
) -> Json<u64> {
    let page = query.into_inner();
    Json(item.a + item.b + item.c + page.after.unwrap_or(0) + u64::from(page.per_page))
}

/// `echo-post`, its body one of two representations, sent the first.
#[kynos::post("/one-of")]
async fn one_of(body: OneOf<Json<Echo>, Form<Echo>>) -> Json<Echo> {
    match body {
        OneOf::Left(Json(echo)) | OneOf::Right(Form(echo)) => Json(echo),
    }
}

/// `plaintext`'s answer, to a request carrying it as a text body.
#[kynos::post("/text")]
async fn text_body(Text(text): Text) -> Text {
    Text(text)
}

/// Mounts this group's handlers.
pub(crate) fn mount(router: Router<App>) -> Router<App> {
    router.mount(kynos::routes![
        inject,
        auth_bearer,
        auth_basic,
        auth_api_key,
        cookies,
        bounded,
        querystring,
        one_of,
        text_body
    ])
}
