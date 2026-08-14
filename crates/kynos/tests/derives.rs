//! Every derive expands to a well-formed implementation.
//!
//! Bodies are still `todo!()`, so nothing here runs one. What is checked is
//! that the expansion *compiles* and that the trait is actually implemented —
//! which is the difference between a derive that emits a placeholder body and
//! one that panics the compiler at expansion time. Until that difference
//! existed, no example, doctest or compile-fail case could name a user type at
//! all.

#![cfg(feature = "macros")]
#![allow(dead_code)]

use kynos::{
    ApiError, HeaderParams, PathParams, QueryParams, Reply, Schema, SecurityScheme, Tag,
    extract::params::{
        header::HeaderParams, path::PathParams as PathParamsTrait,
        query::QueryParams as QueryParamsTrait,
    },
    response::{IntoResponse, Responses},
    router::operation::Tag as TagTrait,
    schema::Schema as SchemaTrait,
    security::SecurityScheme as SecuritySchemeTrait,
};

#[derive(Schema)]
struct User {
    id: u64,
    name: String,
}

/// An internally tagged enum, which is the shape that becomes a
/// `discriminator`. The derive reads serde's attributes rather than asking for
/// the same facts twice.
#[derive(Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Shape {
    Circle { radius: f64 },
    Square { side: f64 },
}

/// A generic type still mangles to one component name, so the derive must
/// carry the generics through rather than assuming there are none.
#[derive(Schema)]
struct Page<T> {
    items: Vec<T>,
    total: u64,
}

#[derive(PathParams)]
struct UserPath {
    user_id: u64,
}

#[derive(Schema, QueryParams)]
struct ListQuery {
    page: u32,
    #[param(rename = "per_page")]
    per: u32,
}

#[derive(HeaderParams)]
struct Conditional {
    #[header(rename = "If-None-Match")]
    if_none_match: String,
}

#[cfg(feature = "cookie")]
#[derive(kynos::CookieParams)]
struct Session {
    #[cookie(rename = "session_id")]
    session: String,
}

#[derive(Tag)]
#[tag(name = "users", description = "Managing user accounts")]
struct Users;

/// With no `name`, the tag is its own identifier — so there is no string to
/// misspell.
#[derive(Tag)]
struct Admin;

#[derive(SecurityScheme)]
#[security(bearer, name = "BearerAuth", credential = String)]
struct BearerAuth;

/// Cookie authentication needs no new type and no `cookie` feature: a scheme
/// is pure description, and the authenticator parses the header itself.
#[derive(SecurityScheme)]
#[security(api_key(in = "cookie", name = "session"))]
#[security(name = "SessionCookie")]
struct SessionCookie;

/// The whole `#[problem(...)]` grammar, so the expansion is exercised by a
/// compiled use rather than only by compile-fail cases.
///
/// `detail` comes from `Display`, which is why `thiserror` sits alongside: the
/// `#[error("...")]` a Rust reader sees is the sentence an API consumer gets.
#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum StoreError {
    #[error("no user with id {id}")]
    #[problem(status = 404, title = "User not found")]
    NotFound {
        #[problem(extension)]
        id: u64,
        trace: String,
    },

    #[error("that email is already registered")]
    #[problem(status = 409, type = "https://errors.example.com/email-taken")]
    Conflict,
}

#[derive(Reply)]
enum CreateReply {
    #[reply(status = 201, description = "the user as stored")]
    Created(User),
    #[reply(status = 409)]
    Conflict,
}

fn implements_schema<T: SchemaTrait>() {}
fn implements_path_params<T: PathParamsTrait>() {}
fn implements_query_params<T: QueryParamsTrait>() {}
fn implements_header_params<T: HeaderParams>() {}
#[cfg(feature = "cookie")]
fn implements_cookie_params<T: kynos::extract::params::cookie::CookieParams>() {}
fn implements_tag<T: TagTrait>() {}
fn implements_security_scheme<T: SecuritySchemeTrait>() {}
fn implements_responses<T: IntoResponse + Responses>() {}

#[test]
fn every_derive_implements_its_trait() {
    implements_schema::<User>();
    implements_schema::<Shape>();
    implements_schema::<Page<u32>>();
    implements_path_params::<UserPath>();
    implements_query_params::<ListQuery>();
    implements_header_params::<Conditional>();
    #[cfg(feature = "cookie")]
    implements_cookie_params::<Session>();
    implements_tag::<Users>();
    implements_tag::<Admin>();
    implements_security_scheme::<BearerAuth>();
    implements_security_scheme::<SessionCookie>();
    implements_responses::<StoreError>();
    implements_responses::<CreateReply>();
}

#[test]
fn declared_names_reach_the_trait_constants() {
    assert_eq!(<UserPath as PathParamsTrait>::NAMES, ["user_id"]);
    assert_eq!(<Conditional as HeaderParams>::NAMES, ["If-None-Match"]);
    #[cfg(feature = "cookie")]
    assert_eq!(
        <Session as kynos::extract::params::cookie::CookieParams>::NAMES,
        ["session_id"]
    );
    assert_eq!(<Users as TagTrait>::NAME, "users");
    assert_eq!(<BearerAuth as SecuritySchemeTrait>::NAME, "BearerAuth");
    assert_eq!(
        <SessionCookie as SecuritySchemeTrait>::NAME,
        "SessionCookie"
    );
}

/// A tag with no explicit name takes the type's own identifier.
#[test]
fn a_tag_defaults_to_its_type_name() {
    assert_eq!(<Admin as TagTrait>::NAME, "Admin");
}

/// A named type is registered as a component rather than inlined.
#[test]
fn a_derived_schema_claims_a_component_name() {
    assert_eq!(
        <User as SchemaTrait>::name().map(|name| name.as_str().to_owned()),
        Some("User".to_owned())
    );
}

#[derive(Clone, Debug, PartialEq)]
struct Pool(u32);

#[derive(Clone, Debug, PartialEq)]
struct Cache(&'static str);

#[derive(kynos::Provider)]
struct App {
    pool: Pool,
    cache: Cache,
    /// Not every field is a dependency, and opting one out must not need a
    /// newtype.
    #[provide(skip)]
    #[allow(dead_code)]
    name: &'static str,
}

fn provides<C: kynos::di::Provides<Pool> + kynos::di::Provides<Cache>>(
    context: &C,
) -> (Pool, Cache) {
    (context.provide(), context.provide())
}

#[test]
fn the_provider_derive_supplies_every_field_it_was_not_told_to_skip() {
    let app = App {
        pool: Pool(7),
        cache: Cache("local"),
        name: "orders",
    };
    assert_eq!(provides(&app), (Pool(7), Cache("local")));
}

/// An HTTP authentication scheme knows its own challenge, so a 401 and the
/// description cannot disagree about what a client should do next.
#[test]
fn an_http_scheme_supplies_its_challenge() {
    assert_eq!(
        <BearerAuth as SecuritySchemeTrait>::challenge(),
        Some("Bearer")
    );
    assert_eq!(<SessionCookie as SecuritySchemeTrait>::challenge(), None);
}

/// The derives, counted against the entry points that declare them.
///
/// `every_derive_implements_its_trait` witnesses a set someone chose, and
/// nothing tied that set to the macros the crate actually exports. A derive
/// added without a witness is one that could expand to anything -- and eight
/// witnesses against ten entry points is the state this test was written to
/// end.
///
/// A count rather than a mapping, like `every_rejected_schema_type_has_a_case`:
/// it catches a derive added without a witness, and not a witness renamed to
/// cover a different one.
#[test]
fn every_derive_has_a_witness() {
    const SOURCE: &str = include_str!("../../kynos-macros/src/lib.rs");

    /// Every derive witnessed in this file. `Provider` is exercised by
    /// `the_provider_derive_supplies_every_field_it_was_not_told_to_skip`,
    /// `ApiError` and `Reply` through `implements_responses`, and the rest by
    /// `every_derive_implements_its_trait`.
    const WITNESSED: &[&str] = &[
        "ApiError",
        "CookieParams",
        "HeaderParams",
        "PathParams",
        "Provider",
        "QueryParams",
        "Reply",
        "Schema",
        "SecurityScheme",
        "Tag",
    ];

    let declared = SOURCE.matches("#[proc_macro_derive(").count();
    assert_eq!(
        declared,
        WITNESSED.len(),
        "`kynos-macros` declares {declared} derive(s) and {} are witnessed; a derive added \
         without one is a derive nothing asks to implement its trait",
        WITNESSED.len()
    );
}
