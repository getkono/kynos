//! Everything a handler can read from a request head, including a hand-written
//! extractor.
//!
//! Run it with cookie parameters on:
//!
//! ```text
//! cargo run -p kynos --example parameters --features cookie
//! ```
//!
//! Four things are worth noticing:
//!
//! * **Every argument declares itself.** There is no `Request`, no `HeaderMap`,
//!   no `Body`. An operation cannot read something its description never
//!   mentions, because the only way in is a type that says what it reads.
//! * **Some arguments declare that they read nothing.** `MatchedPath` and
//!   `ConnectInfo` implement `Describe` with an empty body. That is a claim a
//!   reviewer can see, not a step somebody skipped: a consumer cannot observe
//!   either of them, so the description must not mention them.
//! * **Three header names are refused.** `Accept`, `Content-Type` and
//!   `Authorization` are compile errors in a `HeaderParams` group, because the
//!   specification says a parameter definition for them is ignored — a claim no
//!   consumer will honour is worse than no claim. The diagnostic names the
//!   right tool for each.
//! * **A hand-written extractor is a first-class one.** `ApiVersion` below
//!   implements the same two traits the derives implement. Nothing about the
//!   derived path is privileged, which is what makes the rule "every argument
//!   describes itself" enforceable rather than aspirational.

use std::net::Ipv4Addr;

use kynos::{
    extract::{
        FromRequestParts,
        connection::{ConnectInfo, MatchedPath},
        describe::Describe,
        params::{cookie::Cookies, header::Headers},
    },
    http::Parts,
    openapi::{Parameter, ParameterIn},
    prelude::*,
    router::operation::OperationCx,
    server::Server,
};
use serde::{Deserialize, Serialize};

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

/// What `/users/{id}` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct UserPath {
    /// A path parameter's name must match the template's variable, and the
    /// derive checks that against `EndpointMeta::PATH_VARIABLES` at compile
    /// time rather than at startup.
    id: u64,
}

/// How a list is paged.
#[allow(dead_code)]
#[derive(Schema, QueryParams)]
struct Page {
    /// Optional, because the type is.
    after: Option<u64>,

    /// `rename` is what lets a Rust name and a wire name differ without either
    /// being written twice. Both derives read serde's `rename` too, so a type
    /// that already carries one does not repeat itself.
    #[param(rename = "per_page")]
    per: u32,
}

/// Conditional-request headers.
///
/// A group rather than one argument per header, so the operation's parameter
/// list and the handler's destructuring come from a single declaration.
#[allow(dead_code)]
#[derive(HeaderParams)]
struct Conditional {
    #[header(rename = "If-None-Match")]
    if_none_match: Option<String>,

    #[header(rename = "If-Modified-Since")]
    if_modified_since: Option<String>,
}

/// A session cookie that is *not* a credential.
///
/// A cookie carrying credentials is a `SecurityScheme`, not a parameter, which
/// is why there is no whole-jar extractor to reach for. This one carries a
/// display preference.
#[allow(dead_code)]
#[derive(kynos::CookieParams)]
struct Preferences {
    #[cookie(rename = "ui_theme")]
    theme: String,
}

/// The API version a client asked for, read from a custom header.
///
/// Hand-written to show that the derives are a convenience rather than a
/// privilege: this implements exactly the two traits they implement, and a
/// handler cannot tell the difference.
///
/// Header-based versioning is a documented anti-pattern — it hides the version
/// from a URL, a cache key and a browser address bar. This is here as the
/// mechanism, not as a recommendation.
struct ApiVersion(u32);

impl<C: Sync> FromRequestParts<C> for ApiVersion {
    /// The rejection is where the operation's 400 comes from. An extractor
    /// never lists its own failure responses; `Handler::describe` unions them,
    /// which is why one cannot be forgotten.
    type Rejection = kynos::error::rejection::HeaderRejection;

    async fn from_request_parts(parts: &mut Parts, context: &C) -> Result<Self, Self::Rejection> {
        let _ = (parts, context);
        todo!("the router is still a skeleton; this example exists to typecheck")
    }
}

impl Describe for ApiVersion {
    fn describe(operation: &mut OperationCx<'_>) {
        let schema = <u32 as kynos::schema::Schema>::schema(operation.registry());
        let mut parameter = Parameter::new("X-Api-Version", ParameterIn::Header, schema);
        parameter.description = Some("The API version this client was written against".to_owned());
        parameter.required = Some(true);
        operation.add_parameter(parameter);
    }
}

/// Fetches one user.
///
/// Seven arguments, and the description lists exactly the six things a consumer
/// can observe: one path variable, two query parameters, two conditional
/// headers, one cookie and one custom header. `MatchedPath` and `ConnectInfo`
/// contribute nothing, by saying so.
#[kynos::get("/users/{id}")]
#[allow(clippy::too_many_arguments)]
async fn get_user(
    Path(path): Path<UserPath>,
    Query(page): Query<Page>,
    Headers(conditional): Headers<Conditional>,
    Cookies(preferences): Cookies<Preferences>,
    version: ApiVersion,
    matched: MatchedPath,
    peer: ConnectInfo,
) -> Json<User> {
    // `matched` is the `paths` key with its `{}` intact, never the request's
    // own path, which is what makes it a bounded metric label.
    let _ = (
        path,
        page,
        conditional,
        preferences,
        version.0,
        matched.0,
        peer.0,
    );
    todo!("the router is still a skeleton; this example exists to typecheck")
}

#[tokio::main]
async fn main() -> kynos::Result<()> {
    let router = Router::<()>::new().mount(kynos::routes![get_user]);

    let document = router.openapi()?;
    println!("{}", document.to_json()?);

    Server::new(router.build(())?)
        .bind((Ipv4Addr::UNSPECIFIED, 3000))
        .serve()
        .await
}
