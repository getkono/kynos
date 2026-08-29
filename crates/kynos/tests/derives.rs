//! Every derive expands to a well-formed implementation.
//!
//! What is checked is that each expansion *compiles* and that the trait is
//! actually implemented — which is the difference between a derive that emits a
//! well-formed body and one that panics the compiler at expansion time. Until
//! that difference existed, no example, doctest or compile-fail case could name
//! a user type at all.
//!
//! What a derived decoder then *does* is not checked here. That is the macro
//! crate's, and `docs/testing.md` allocates it there.

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

#[derive(Schema, serde::Serialize)]
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

/// A multipart body travels in both directions from one declaration, and each
/// arity a field's type can declare is exercised: one part, none or one, and
/// one per element.
#[cfg(feature = "multipart")]
#[derive(Schema, kynos::MultipartForm)]
struct Upload {
    name: String,
    caption: Option<String>,
    images: Vec<kynos::extract::body::multipart::FilePart>,
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
#[cfg(feature = "multipart")]
fn implements_multipart<
    T: kynos::extract::body::multipart::FromMultipart
        + kynos::response::codec::multipart::IntoMultipart,
>() {
}

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
    #[cfg(feature = "multipart")]
    implements_multipart::<Upload>();
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

// The count that ties the witnesses above to the macros `kynos-macros`
// actually declares lives in `ledger.rs`, which reads the sibling crate's
// source. That read leaves this package, so the target carrying it is excluded
// from the published archive rather than shipped unable to run.

// --- `#[deprecated]` reaching the description -------------------------------
//
// The derive reads Rust's own attribute rather than a `#[schema(deprecated)]`
// key of its own, so the compiler's warning and the description's keyword
// cannot disagree. These pin the emitted shape; `docs/schema.md` states the
// rule.

/// A shape retired in favour of something else.
///
/// Deliberately carries no `#[allow(deprecated)]` of its own: deriving `Schema`
/// on a deprecated type must not warn at the type's own definition, and this is
/// where that is checked. It fails to compile under `-D warnings` if the
/// derive stops emitting the allow inside its impl.
#[deprecated(note = "the note addresses a Rust caller and never reaches the description")]
#[derive(Schema, serde::Serialize)]
struct RetiredShape {
    id: u64,
}

/// Carries one field nobody should send any more.
#[derive(Schema, serde::Serialize)]
struct PartlyRetired {
    id: u64,
    #[deprecated]
    legacy_name: String,
}

/// An internally tagged enum with one retired branch.
#[derive(Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Settlement {
    Card {
        last4: String,
    },
    #[deprecated]
    Cheque,
}

/// Every variant is a unit, and one of them is retired.
#[derive(Schema, serde::Serialize)]
enum Channel {
    Web,
    #[deprecated]
    Fax,
}

/// Every variant is a unit and none is retired: the compact shape stands.
#[derive(Schema, serde::Serialize)]
enum Currency {
    Gbp,
    Jpy,
}

/// The schema `T` emits, as JSON.
fn emitted<T: SchemaTrait>() -> serde_json::Value {
    let mut registry = kynos::schema::registry::Registry::new();
    serde_json::to_value(T::schema(&mut registry)).expect("a schema serializes")
}

/// A deprecated type says so, and says nothing about the note.
#[test]
#[allow(deprecated)]
fn a_deprecated_type_is_marked() {
    let schema = emitted::<RetiredShape>();

    assert_eq!(schema["deprecated"], serde_json::json!(true));
    assert!(
        !schema.to_string().contains("addresses a Rust caller"),
        "the note reached the description: {schema}"
    );
}

/// A deprecated field is marked on the property, not on the type it borrows.
#[test]
fn a_deprecated_field_is_marked() {
    let schema = emitted::<PartlyRetired>();

    assert_eq!(
        schema["properties"]["legacy_name"]["deprecated"],
        serde_json::json!(true)
    );
    assert_eq!(
        schema["properties"]["id"].get("deprecated"),
        None,
        "a field nobody deprecated carries the keyword"
    );
    assert_eq!(
        schema.get("deprecated"),
        None,
        "one deprecated field deprecated the whole type"
    );
}

/// A deprecated variant is marked on its own branch and no other.
#[test]
fn a_deprecated_variant_is_marked() {
    let schema = emitted::<Settlement>();
    let branches = schema["oneOf"]
        .as_array()
        .expect("a tagged enum is a oneOf");

    let marked: Vec<bool> = branches
        .iter()
        .map(|branch| branch.get("deprecated") == Some(&serde_json::json!(true)))
        .collect();

    assert_eq!(marked, vec![false, true], "{schema}");
}

/// Deprecating one name of an all-unit enum drops the compact shape for one
/// that has somewhere to put the keyword.
///
/// `enum: ["Web", "Fax"]` is one schema shared by every name, so it cannot say
/// that one of them is retired. The `oneOf` of `const` branches describes the
/// same wire values and gives each its own schema. Emitting nothing was the
/// alternative, and it would leave the description disagreeing with the type in
/// the one direction nobody can see.
#[test]
fn deprecating_one_name_gives_every_name_its_own_schema() {
    let schema = emitted::<Channel>();
    let branches = schema["oneOf"].as_array().expect("{schema}");

    assert_eq!(branches.len(), 2);
    assert_eq!(branches[0]["const"], serde_json::json!("Web"));
    assert_eq!(branches[0].get("deprecated"), None);
    assert_eq!(branches[1]["const"], serde_json::json!("Fax"));
    assert_eq!(branches[1]["deprecated"], serde_json::json!(true));
}

/// The control: without a deprecation the compact shape is unchanged.
#[test]
fn an_all_unit_enum_keeps_the_compact_shape() {
    assert_eq!(
        emitted::<Currency>(),
        serde_json::json!({
            "type": "string",
            "enum": ["Gbp", "Jpy"],
            "description": "Every variant is a unit and none is retired: the compact shape stands."
        })
    );
}
