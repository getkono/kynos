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

/// One field of each kind `required` tells apart: always present, optional by
/// its type, and optional because serde's wire form lets it be absent both
/// ways. `skip_serializing_if` is accepted only beside a `default`, which is
/// what makes `elided` absent on read as well as on write.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Draft {
    plain: u64,
    maybe: Option<u64>,
    #[serde(default)]
    defaulted: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    elided: String,
}

/// `required` names only what serde always reads and writes.
///
/// An `Option` may be absent because its type says so, and a
/// `#[serde(default)]` field because serde fills it in on read. Listing either
/// would describe a document the type never demands. The read below holds the
/// other half: a document naming only what `required` lists is one serde
/// accepts, so the schema cannot promise a request body that is refused.
#[test]
fn required_lists_only_what_serde_always_reads_and_writes() {
    assert_eq!(emitted::<Draft>()["required"], serde_json::json!(["plain"]));
    assert!(
        serde_json::from_str::<Draft>(r#"{"plain":1}"#).is_ok(),
        "a document carrying only the required fields must read"
    );
}

/// A container `#[serde(default)]`, which serde honours by filling every
/// missing field from the struct's own `Default`. `label` also carries
/// `skip_serializing_if`, which the container `default` makes absent-tolerant
/// on read as well as on write.
#[derive(Default, Schema, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Settings {
    retries: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    label: String,
}

/// Under a container `default`, no field is required.
///
/// serde reads `{}` into `Settings::default()`, so a `required` list naming
/// any field would call a document invalid that the type accepts, and a
/// `skip_serializing_if` field under it is accepted rather than refused.
#[test]
fn a_container_default_leaves_every_field_optional() {
    assert_eq!(emitted::<Settings>().get("required"), None);
    assert!(
        serde_json::from_str::<Settings>("{}").is_ok(),
        "an empty document must read under a container default"
    );
}

// --- A named field serde skips in one direction -----------------------------
//
// serde reads a `skip_serializing` field and never writes it, so it is a
// property under the `required` rule; it writes a `skip_deserializing` field
// and never reads it, so an object naming only what it reads leaves it out.
// These pin the emitted shape against what serde writes and reads;
// `docs/schema.md` states the rule. None carries a doc comment, which would add
// prose to the shapes compared.

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Unsent {
    plain: u64,
    #[serde(skip_serializing, default)]
    defaulted: u64,
    #[serde(skip_serializing)]
    maybe: Option<u64>,
}

#[derive(Default, Schema, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct UnsentUnderDefault {
    plain: u64,
    #[serde(skip_serializing)]
    elided: u64,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Amendment {
    Now(u64),
    #[serde(skip_serializing)]
    Queued {
        at: u64,
        #[serde(skip_serializing)]
        note: u64,
    },
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Revised {
    id: u64,
    #[serde(skip_serializing, default)]
    revision: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: std::collections::BTreeMap<String, String>,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Annotated {
    id: u64,
    #[serde(flatten, skip_serializing)]
    #[schema(open)]
    extra: std::collections::BTreeMap<String, u64>,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Stamped {
    id: u64,
    #[serde(skip_deserializing)]
    stamp: u64,
}

/// A field serde reads and never writes is a property, left out of `required`
/// where serde reads it absent.
///
/// Left out of the schema, `defaulted` would admit the `"x"` serde refuses.
#[test]
fn a_named_field_serde_reads_and_never_writes_is_an_optional_property() {
    assert_eq!(
        emitted::<Unsent>(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "plain": emitted::<u64>(),
                "defaulted": emitted::<u64>(),
                "maybe": emitted::<Option<u64>>(),
            },
            "required": ["plain"],
        })
    );
    let written = serde_json::to_value(Unsent {
        plain: 1,
        defaulted: 2,
        maybe: Some(3),
    })
    .expect("a struct serializes");
    assert_eq!(written, serde_json::json!({"plain": 1}));
    assert!(serde_json::from_value::<Unsent>(written).is_ok());
    assert!(
        serde_json::from_str::<Unsent>(r#"{"plain":1,"defaulted":"x"}"#).is_err(),
        "serde must refuse the value the property's schema refuses"
    );

    assert_eq!(
        emitted::<UnsentUnderDefault>(),
        serde_json::json!({
            "type": "object",
            "properties": {"plain": emitted::<u64>(), "elided": emitted::<u64>()},
        })
    );
    assert!(serde_json::from_str::<UnsentUnderDefault>("{}").is_ok());
}

/// Inside a variant serde never writes, a field serde never writes is only
/// read, so it is required as any other field serde reads.
#[test]
fn a_named_field_serde_never_writes_is_required_where_its_variant_is_never_written() {
    assert_eq!(
        emitted::<Amendment>()["oneOf"][1]["properties"]["Queued"],
        serde_json::json!({
            "type": "object",
            "properties": {"at": emitted::<u64>(), "note": emitted::<u64>()},
            "required": ["at", "note"],
        })
    );
    assert!(
        serde_json::from_str::<Amendment>(r#"{"Queued":{"at":1}}"#).is_err(),
        "serde must refuse the object `required` refuses"
    );
    assert!(serde_json::from_str::<Amendment>(r#"{"Queued":{"at":1,"note":2}}"#).is_ok());
}

/// Beside an open map, a field serde reads and never writes is named, so the
/// map's `unevaluatedProperties` does not reach it.
///
/// Left out, `revision` would meet the map's string schema, which refuses the
/// `{"id":1,"revision":2}` serde reads.
#[test]
fn a_named_field_serde_never_writes_is_named_beside_an_open_map() {
    let schema = emitted::<Revised>();
    assert_eq!(schema["properties"]["revision"], emitted::<u64>());
    assert_eq!(schema["required"], serde_json::json!(["id"]));
    assert_eq!(schema["unevaluatedProperties"], emitted::<String>());

    let read = serde_json::from_str::<Revised>(r#"{"id":1,"revision":2}"#)
        .expect("serde reads the field beside the map");
    assert_eq!(read.revision, 2);
    assert!(read.extra.is_empty(), "the field reached the map");
}

/// A flattened map serde never writes is still the object's open members on
/// read.
///
/// Left out, the object would admit the `{"id":1,"k":"v"}` serde refuses.
#[test]
fn a_flattened_map_serde_never_writes_is_still_open() {
    let schema = emitted::<Annotated>();
    assert_eq!(schema["unevaluatedProperties"], emitted::<u64>());
    assert_eq!(schema["required"], serde_json::json!(["id"]));

    let written = serde_json::to_value(Annotated {
        id: 1,
        extra: std::collections::BTreeMap::from([("k".to_owned(), 2)]),
    })
    .expect("a struct serializes");
    assert_eq!(written, serde_json::json!({"id": 1}));
    assert!(serde_json::from_str::<Annotated>(r#"{"id":1,"k":2}"#).is_ok());
    assert!(
        serde_json::from_str::<Annotated>(r#"{"id":1,"k":"v"}"#).is_err(),
        "serde must refuse the member `unevaluatedProperties` refuses"
    );
}

/// A field serde writes and never reads is left out of an object that
/// constrains no member it does not name, which admits it written and ignores
/// it read, as serde does.
#[test]
fn a_named_field_serde_never_reads_is_left_out() {
    assert_eq!(
        emitted::<Stamped>(),
        serde_json::json!({
            "type": "object",
            "properties": {"id": emitted::<u64>()},
            "required": ["id"],
        })
    );
    let written = serde_json::to_value(Stamped { id: 1, stamp: 2 }).expect("a struct serializes");
    assert_eq!(written, serde_json::json!({"id": 1, "stamp": 2}));
    assert!(serde_json::from_str::<Stamped>(r#"{"id":1,"stamp":"x"}"#).is_ok());
}

// --- A PhantomData member is the null serde writes and reads ----------------
//
// serde writes a `PhantomData` as `null` and requires it on read like any other
// member, so the schema names it as `null` rather than leaving it out. None
// carries a doc comment, for the reason the section above gives.

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Marked<T> {
    id: u64,
    marker: std::marker::PhantomData<T>,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct MarkedTuple<T>(u64, std::marker::PhantomData<T>);

/// A named `PhantomData` is a required `null` property.
///
/// serde writes `{"id":1,"marker":null}` and refuses `{"id":1}`, which an
/// object leaving the field out admits.
#[test]
fn a_named_phantom_field_is_the_required_null_serde_writes() {
    assert_eq!(
        emitted::<Marked<User>>(),
        serde_json::json!({
            "type": "object",
            "properties": {"id": emitted::<u64>(), "marker": emitted::<()>()},
            "required": ["id", "marker"],
        })
    );

    let written = serde_json::to_value(Marked::<User> {
        id: 1,
        marker: std::marker::PhantomData,
    })
    .expect("a struct serializes");
    assert_eq!(written, serde_json::json!({"id": 1, "marker": null}));
    assert!(serde_json::from_value::<Marked<User>>(written).is_ok());
    assert!(
        serde_json::from_str::<Marked<User>>(r#"{"id":1}"#).is_err(),
        "serde must refuse the document `required` refuses"
    );
}

/// A positional `PhantomData` is the `null` serde writes in its position.
#[test]
fn a_positional_phantom_member_is_the_null_serde_writes() {
    assert_eq!(
        emitted::<MarkedTuple<User>>(),
        serde_json::json!({
            "type": "array",
            "prefixItems": [emitted::<u64>(), emitted::<()>()],
            "items": false,
            "minItems": 2,
        })
    );

    let written = serde_json::to_value(MarkedTuple::<User>(1, std::marker::PhantomData))
        .expect("a tuple serializes");
    assert_eq!(written, serde_json::json!([1, null]));
    assert!(serde_json::from_value::<MarkedTuple<User>>(written).is_ok());
    assert!(
        serde_json::from_str::<MarkedTuple<User>>("[1]").is_err(),
        "serde must refuse the array `minItems` refuses"
    );
}

// --- A transparent struct is the one field serde writes ---------------------
//
// serde writes a `#[serde(transparent)]` struct as its one field's value, so the
// description is that field's schema rather than an object of the fields the
// declaration names. These pin the emitted shape; `docs/schema.md` states the
// rule.

// No doc comment, which would add prose the map's own schema does not carry.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Labels {
    inner: std::collections::BTreeMap<String, String>,
}

/// A name a person reads.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct DisplayName {
    /// The name as it was typed, which the struct's own prose replaces.
    #[schema(min_length = 1, max_length = 10)]
    value: String,
}

// No doc comment, for the reason `Labels` gives.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Handle(u64, #[serde(skip)] u64);

/// A transparent struct is described by the value serde writes.
///
/// `Labels` writes `{"k":"v"}`, which an object requiring `inner` refuses. It
/// keeps its own component name, as a newtype does, so a reference to it names
/// the type that was declared.
#[test]
fn a_transparent_struct_is_described_by_its_one_field() {
    assert_eq!(
        emitted::<Labels>(),
        emitted::<std::collections::BTreeMap<String, String>>()
    );
    assert_eq!(
        <Labels as SchemaTrait>::name().map(|name| name.as_str().to_owned()),
        Some("Labels".to_owned())
    );

    let written = serde_json::to_value(Labels {
        inner: [("k".to_owned(), "v".to_owned())].into(),
    })
    .expect("a map serializes");
    assert_eq!(written, serde_json::json!({"k": "v"}));
    assert!(
        serde_json::from_value::<Labels>(written).is_ok(),
        "the value the schema describes must read back"
    );
}

/// A transparent field's constraints reach the schema, under the struct's prose.
#[test]
fn a_transparent_field_keeps_what_it_said_about_its_value() {
    assert_eq!(
        emitted::<DisplayName>(),
        serde_json::json!({
            "type": "string",
            "minLength": 1,
            "maxLength": 10,
            "description": "A name a person reads."
        })
    );

    let written = serde_json::to_value(DisplayName {
        value: "Ada".to_owned(),
    })
    .expect("a string serializes");
    assert_eq!(written, serde_json::json!("Ada"));
    assert!(
        serde_json::from_value::<DisplayName>(written).is_ok(),
        "the value the schema describes must read back"
    );
}

/// A transparent tuple struct is its one described member, not an array of all.
#[test]
fn a_transparent_tuple_struct_is_described_by_its_one_described_member() {
    assert_eq!(emitted::<Handle>(), emitted::<u64>());
    assert_eq!(
        serde_json::to_value(Handle(1, 2)).expect("an integer serializes"),
        serde_json::json!(1)
    );
    assert!(
        serde_json::from_value::<Handle>(serde_json::json!(1)).is_ok(),
        "the value the schema describes must read back"
    );
}

// No doc comment, so the field's prose is the only prose there is.
#[derive(Schema, serde::Serialize)]
#[serde(transparent)]
struct Nickname {
    /// What a person goes by.
    #[deprecated]
    value: String,
}

/// With no prose of its own, a transparent struct publishes its field's prose
/// and its field's deprecation.
#[test]
fn a_transparent_field_publishes_its_prose_and_deprecation() {
    assert_eq!(
        emitted::<Nickname>(),
        serde_json::json!({
            "type": "string",
            "description": "What a person goes by.",
            "deprecated": true
        })
    );
}

// No doc comment, for the reason `Labels` gives.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Code(#[schema(min_length = 1)] String);

/// A transparent tuple struct carries its member's constraints, as its
/// named-field twin does.
#[test]
fn a_transparent_tuple_member_keeps_its_constraints() {
    assert_eq!(
        emitted::<Code>(),
        serde_json::json!({"type": "string", "minLength": 1})
    );
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Wrapped<T> {
    value: T,
}

/// A generic transparent struct claims no component name, which every
/// instantiation would otherwise share.
#[test]
fn a_generic_transparent_struct_claims_no_component_name() {
    assert!(
        <Wrapped<u64> as SchemaTrait>::name().is_none(),
        "a generic transparent struct claimed a component name"
    );
}

// No doc comment, for the reason `Labels` gives. serde writes this through both
// fields, so it refuses `Serialize` here, and reads it through `value` alone.
#[derive(Schema, serde::Deserialize)]
#[serde(transparent)]
struct Reading {
    value: u64,
    #[serde(default)]
    extra: String,
}

/// A transparent struct serde derives in one direction only is described by
/// the one field that direction picks.
#[test]
fn a_transparent_struct_read_through_one_field_is_described_by_it() {
    assert_eq!(emitted::<Reading>(), emitted::<u64>());

    let read = serde_json::from_value::<Reading>(serde_json::json!(5))
        .expect("the value the schema describes must read");
    assert_eq!(read.value, 5);
}

// --- A tuple is the positions serde writes and reads ------------------------
//
// serde leaves a member it skips both ways out of the array in both directions,
// and writes a newtype variant whose member it skips as a unit variant. These
// pin the emitted shape against what serde writes and reads; `docs/schema.md`
// states the rule. None carries a doc comment, for the reason `Labels` gives.

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Late(#[serde(skip)] u64, String);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Pair(u64, String);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Tally(u64, #[serde(default, skip_serializing_if = "is_zero")] u64);

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes the field by reference
fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Blank(#[serde(skip)] u64, #[serde(skip)] String);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Count(#[serde(skip_serializing)] u64);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum External {
    Shown(u64),
    Hidden(#[serde(skip)] u64),
}

// An `Option` member, because serde reads an adjacently tagged variant's missing
// content only as an absent `Option`; any other member type is refused.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t", content = "c")]
enum Adjacent {
    Shown(u64),
    Hidden(#[serde(skip)] Option<u64>),
}

#[derive(Default, Schema, serde::Serialize, serde::Deserialize)]
struct Payload {
    x: u64,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t")]
enum Internal {
    Shown(Payload),
    Hidden(#[serde(skip)] Payload),
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Units {
    A,
    B(#[serde(skip)] u64),
}

/// A tag-only object: what a tagged unit variant is on the wire.
fn tag_only(tag: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {tag: {"type": "string", "const": name}},
        "required": [tag],
    })
}

/// A member skipped both ways is no position, so the later ones keep theirs.
///
/// `Late` writes `["s"]`, which an array whose first position is an integer
/// refuses.
#[test]
fn a_tuple_is_the_members_serde_writes_and_reads() {
    assert_eq!(
        emitted::<Late>(),
        serde_json::json!({
            "type": "array",
            "prefixItems": [emitted::<String>()],
            "items": false,
            "minItems": 1,
        })
    );

    let written = serde_json::to_value(Late(7, "s".to_owned())).expect("a tuple serializes");
    assert_eq!(written, serde_json::json!(["s"]));
    assert!(
        serde_json::from_value::<Late>(written).is_ok(),
        "the array the schema describes must read back"
    );
}

/// `minItems` refuses the shorter array serde refuses, which `prefixItems`
/// alone admits.
#[test]
fn a_tuple_admits_no_fewer_members_than_serde_reads() {
    assert_eq!(
        emitted::<Pair>(),
        serde_json::json!({
            "type": "array",
            "prefixItems": [emitted::<u64>(), emitted::<String>()],
            "items": false,
            "minItems": 2,
        })
    );
    assert!(
        serde_json::from_str::<Pair>("[1]").is_err(),
        "serde must refuse the array `minItems` refuses"
    );
    assert!(serde_json::from_str::<Pair>(r#"[1,"s"]"#).is_ok());
}

/// A last member serde may leave out, and fills from `Default` when the array
/// ends before it, lowers the bound by one.
#[test]
fn a_trailing_member_serde_may_leave_out_lowers_min_items() {
    assert_eq!(
        emitted::<Tally>(),
        serde_json::json!({
            "type": "array",
            "prefixItems": [emitted::<u64>(), emitted::<u64>()],
            "items": false,
            "minItems": 1,
        })
    );

    let written = serde_json::to_value(Tally(1, 0)).expect("a tuple serializes");
    assert_eq!(written, serde_json::json!([1]));
    assert!(
        serde_json::from_value::<Tally>(written).is_ok(),
        "the shorter array serde writes must read back"
    );
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Filled(u64, #[serde(default)] u64, #[serde(default)] u64);

// serde refuses a default before a required member on a tuple struct, but not
// on a tuple variant.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Gauge {
    Read(#[serde(default)] u64, u64, #[serde(default)] u64),
}

/// serde fills each member carrying `#[serde(default)]` when the array ends
/// before it, so the bound counts up to the last member with no default: a
/// default before that one fills nothing, since the array cannot end there.
#[test]
fn a_trailing_run_of_defaulted_members_lowers_min_items_to_the_last_required_one() {
    assert_eq!(
        emitted::<Filled>(),
        serde_json::json!({
            "type": "array",
            "prefixItems": [emitted::<u64>(), emitted::<u64>(), emitted::<u64>()],
            "items": false,
            "minItems": 1,
        })
    );
    assert!(
        serde_json::from_str::<Filled>("[1]").is_ok(),
        "serde fills the trailing defaulted members"
    );
    assert!(serde_json::from_str::<Filled>("[]").is_err());

    assert_eq!(
        emitted::<Gauge>()["oneOf"][0]["properties"]["Read"],
        serde_json::json!({
            "type": "array",
            "prefixItems": [emitted::<u64>(), emitted::<u64>(), emitted::<u64>()],
            "items": false,
            "minItems": 2,
        })
    );
    assert!(
        serde_json::from_str::<Gauge>(r#"{"Read":[1,2]}"#).is_ok(),
        "serde fills the trailing defaulted member"
    );
    assert!(
        serde_json::from_str::<Gauge>(r#"{"Read":[1]}"#).is_err(),
        "a default before a required member fills nothing"
    );
}

/// With every member skipped, the tuple is the empty array, and there is no
/// `prefixItems`, which may not be empty.
#[test]
fn a_tuple_whose_every_member_is_skipped_is_the_empty_array() {
    assert_eq!(
        emitted::<Blank>(),
        serde_json::json!({"type": "array", "items": false})
    );

    let written =
        serde_json::to_value(Blank(7, "s".to_owned())).expect("an empty tuple serializes");
    assert_eq!(written, serde_json::json!([]));
    assert!(serde_json::from_value::<Blank>(written).is_ok());
}

/// serde ignores skip attributes on a newtype struct, so its member stands.
#[test]
fn a_newtype_struct_is_its_member_whatever_serde_skips() {
    assert_eq!(emitted::<Count>(), emitted::<u64>());

    let written = serde_json::to_value(Count(5)).expect("a newtype serializes");
    assert_eq!(written, serde_json::json!(5));
    assert!(serde_json::from_value::<Count>(written).is_ok());
}

/// A newtype variant whose member serde skips is the unit variant serde writes,
/// under every tagging.
#[test]
fn a_newtype_variant_whose_member_is_skipped_is_a_unit_variant() {
    let hidden = serde_json::json!({"type": "string", "const": "Hidden"});
    assert_eq!(emitted::<External>()["oneOf"][1], hidden);
    let written = serde_json::to_value(External::Hidden(7)).expect("a variant serializes");
    assert_eq!(written, serde_json::json!("Hidden"));
    assert!(serde_json::from_value::<External>(written).is_ok());

    assert_eq!(
        emitted::<Adjacent>()["oneOf"][1],
        tag_only("t", "Hidden"),
        "the branch carries no content property"
    );
    let written = serde_json::to_value(Adjacent::Hidden(Some(7))).expect("a variant serializes");
    assert_eq!(written, serde_json::json!({"t": "Hidden"}));
    assert!(
        serde_json::from_value::<Adjacent>(written).is_ok(),
        "the tag-only branch must read back"
    );

    assert_eq!(
        emitted::<Internal>()["oneOf"][1],
        tag_only("t", "Hidden"),
        "the branch composes no payload"
    );
    let written =
        serde_json::to_value(Internal::Hidden(Payload::default())).expect("a variant serializes");
    assert_eq!(written, serde_json::json!({"t": "Hidden"}));
    assert!(serde_json::from_value::<Internal>(written).is_ok());
}

/// A skipped newtype variant is a name like any unit variant, so an enum of
/// names keeps the compact shape.
#[test]
fn an_enum_of_names_counts_a_skipped_newtype_variant_as_a_name() {
    assert_eq!(
        emitted::<Units>(),
        serde_json::json!({"type": "string", "enum": ["A", "B"]})
    );
    assert_eq!(
        serde_json::to_value(Units::B(7)).expect("a variant serializes"),
        serde_json::json!("B")
    );
}

#[derive(Default, Schema, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Settled(u64, #[serde(skip_serializing_if = "is_zero")] u64);

/// A container `#[serde(default)]` fills every trailing element serde finds
/// missing, so a tuple under it admits any shorter array, down to the empty one.
#[test]
fn a_tuple_under_a_container_default_carries_no_min_items() {
    assert_eq!(
        emitted::<Settled>(),
        serde_json::json!({
            "type": "array",
            "prefixItems": [emitted::<u64>(), emitted::<u64>()],
            "items": false,
        })
    );

    let written = serde_json::to_value(Settled(1, 0)).expect("a tuple serializes");
    assert_eq!(written, serde_json::json!([1]));
    assert!(serde_json::from_value::<Settled>(written).is_ok());
    assert!(
        serde_json::from_str::<Settled>("[]").is_ok(),
        "serde fills the empty array from the container's default"
    );
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Pick(u64, #[serde(default, skip_serializing)] u64);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Meter {
    Tally(u64, #[serde(default, skip_serializing_if = "is_zero")] u64),
}

/// A transparent tuple is its picked member, whatever its other member skips
/// one way: the one-way skip refusal is not reached, and nothing else is
/// described.
#[test]
fn a_transparent_tuple_ignores_a_one_way_skip_on_its_unpicked_member() {
    assert_eq!(emitted::<Pick>(), emitted::<u64>());

    let written = serde_json::to_value(Pick(1, 2)).expect("a transparent tuple serializes");
    assert_eq!(written, serde_json::json!(1));
    assert!(serde_json::from_value::<Pick>(written).is_ok());
}

/// A tuple variant's trailing member serde may leave out lowers its bound, as a
/// tuple struct's does.
#[test]
fn a_tuple_variant_lowers_min_items_for_a_trailing_member_serde_may_leave_out() {
    assert_eq!(
        emitted::<Meter>()["oneOf"][0]["properties"]["Tally"],
        serde_json::json!({
            "type": "array",
            "prefixItems": [emitted::<u64>(), emitted::<u64>()],
            "items": false,
            "minItems": 1,
        })
    );

    let written = serde_json::to_value(Meter::Tally(1, 0)).expect("a variant serializes");
    assert_eq!(written, serde_json::json!({"Tally": [1]}));
    assert!(
        serde_json::from_value::<Meter>(written).is_ok(),
        "the shorter array serde writes must read back"
    );
}

// `Duration` has no schema and `String` is not `Flatten`, so both were compile
// errors while the derive still described the member serde skips.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Elapsed(u64, #[serde(skip)] std::time::Duration);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t")]
enum Remark {
    Shown(Payload),
    Hidden(#[serde(skip)] String),
}

/// A member serde never writes or reads asks nothing of its type.
#[test]
fn a_skipped_member_needs_no_schema_of_its_own() {
    implements_schema::<Elapsed>();
    implements_schema::<Remark>();
}

// --- An unnamed member keeps what it declares -------------------------------
//
// A newtype, tuple or variant member is described under its own constraints,
// prose and `#[deprecated]`, as a named field is. These pin the emitted shape;
// `docs/schema.md` states the rule. None but `Motto` carries a doc comment, for
// the reason `Labels` gives.

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Slug(#[schema(min_length = 1, max_length = 10)] String);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Label(String);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Alias(#[schema(max_length = 8)] Label);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
struct Grid(
    #[serde(skip)]
    #[schema(minimum = 5)]
    u64,
    #[schema(minimum = 1)] u64,
    #[schema(max_length = 2)] String,
);

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Measure {
    Range(#[schema(minimum = 1)] u64, u64),
    Title(#[schema(min_length = 1)] String),
}

#[derive(Schema, serde::Serialize)]
struct Motto(
    /// What the team says.
    #[deprecated]
    String,
);

/// A newtype's constraints reach its schema, beside the `$ref` when its member
/// names a component.
#[test]
fn a_newtype_member_keeps_its_constraints() {
    assert_eq!(
        emitted::<Slug>(),
        serde_json::json!({"type": "string", "minLength": 1, "maxLength": 10})
    );
    let written = serde_json::to_value(Slug("ada".to_owned())).expect("a newtype serializes");
    assert_eq!(written, serde_json::json!("ada"));
    assert!(serde_json::from_value::<Slug>(written).is_ok());

    assert_eq!(
        emitted::<Alias>(),
        serde_json::json!({"$ref": "#/components/schemas/Label", "maxLength": 8})
    );
}

/// Each position carries its own member's constraints, and a member serde skips
/// both ways carries none anywhere.
#[test]
fn a_tuple_member_keeps_its_constraints_at_its_position() {
    assert_eq!(
        emitted::<Grid>(),
        serde_json::json!({
            "type": "array",
            "prefixItems": [
                {"type": "integer", "format": "uint64", "minimum": 1.0},
                {"type": "string", "maxLength": 2},
            ],
            "items": false,
            "minItems": 2,
        })
    );

    let written = serde_json::to_value(Grid(9, 1, "ab".to_owned())).expect("a tuple serializes");
    assert_eq!(written, serde_json::json!([1, "ab"]));
    assert!(serde_json::from_value::<Grid>(written).is_ok());
}

/// A tuple variant's members and a newtype variant's payload keep their
/// constraints inside the branch.
#[test]
fn a_variant_member_keeps_its_constraints() {
    let schema = emitted::<Measure>();
    assert_eq!(
        schema["oneOf"][0]["properties"]["Range"]["prefixItems"][0]["minimum"],
        serde_json::json!(1.0)
    );
    assert_eq!(
        schema["oneOf"][1]["properties"]["Title"],
        serde_json::json!({"type": "string", "minLength": 1})
    );
}

/// A newtype publishes its member's prose and deprecation, as a named field's
/// property does.
#[test]
fn a_newtype_member_publishes_its_prose_and_deprecation() {
    assert_eq!(
        emitted::<Motto>(),
        serde_json::json!({
            "type": "string",
            "description": "What the team says.",
            "deprecated": true
        })
    );
}

// --- A variant serde skips in one direction ---------------------------------
//
// serde reads a `skip_serializing` variant and never writes it, so it is
// described as the name or branch serde reads; a variant serde skips both ways
// is described nowhere. These pin the emitted shape against what serde reads;
// `docs/schema.md` states the rule. None carries a doc comment, for the reason
// `Labels` gives.

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Intake {
    Web,
    #[serde(skip_serializing)]
    Fax,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t")]
enum Command {
    Start {
        at: u64,
    },
    #[serde(skip_serializing)]
    Legacy {
        at: u64,
    },
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Retiring {
    Web,
    #[deprecated]
    #[serde(skip_serializing)]
    Fax,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Withdrawn {
    Web,
    #[serde(skip_serializing)]
    #[serde(skip_deserializing)]
    Fax,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Hidden {
    Web,
    #[deprecated]
    #[serde(skip)]
    Fax,
}

#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Batch {
    Now(u64),
    #[serde(skip_serializing)]
    Queued(u64, #[serde(skip_serializing_if = "is_zero")] u64),
}

/// A variant serde reads and never writes is the name or branch serde reads.
#[test]
fn a_variant_serde_reads_but_never_writes_is_described() {
    assert_eq!(
        emitted::<Intake>(),
        serde_json::json!({"type": "string", "enum": ["Web", "Fax"]})
    );
    assert!(
        serde_json::to_value(Intake::Fax).is_err(),
        "serde must refuse to write the variant"
    );
    assert!(serde_json::from_str::<Intake>(r#""Fax""#).is_ok());

    let branch = |name: &str| {
        serde_json::json!({
            "type": "object",
            "properties": {
                "t": {"type": "string", "const": name},
                "at": emitted::<u64>(),
            },
            "required": ["t", "at"],
        })
    };
    assert_eq!(
        emitted::<Command>(),
        serde_json::json!({
            "oneOf": [branch("Start"), branch("Legacy")],
            "discriminator": {"propertyName": "t"},
        })
    );
    assert!(serde_json::from_str::<Command>(r#"{"t":"Legacy","at":1}"#).is_ok());
}

/// A tuple variant serde never writes admits no fewer members than serde reads:
/// a trailing `skip_serializing_if` with no `#[serde(default)]` lowers nothing,
/// since serde refuses the shorter array.
#[test]
fn a_tuple_variant_serde_never_writes_admits_no_fewer_members_than_it_reads() {
    let schema = emitted::<Batch>();
    let queued = &schema["oneOf"][1]["properties"]["Queued"];
    assert_eq!(queued["minItems"], serde_json::json!(2), "{schema}");
    assert!(serde_json::from_str::<Batch>(r#"{"Queued":[1]}"#).is_err());
    assert!(serde_json::from_str::<Batch>(r#"{"Queued":[1,0]}"#).is_ok());
}

/// A deprecated variant serde only reads keeps its branch and its deprecation.
#[test]
fn a_deprecated_variant_serde_only_reads_is_marked() {
    assert_eq!(
        emitted::<Retiring>(),
        serde_json::json!({"oneOf": [
            {"type": "string", "const": "Web"},
            {"type": "string", "const": "Fax", "deprecated": true},
        ]})
    );
}

/// A variant serde skips both ways, however it is spelt, is in no schema, and
/// neither is its deprecation.
#[test]
fn a_variant_serde_skips_both_ways_is_described_nowhere() {
    let names = serde_json::json!({"type": "string", "enum": ["Web"]});
    assert_eq!(emitted::<Withdrawn>(), names);
    assert_eq!(emitted::<Hidden>(), names);
}

// --- What a derived error response declares ---------------------------------
//
// A problem body carries the type URI the declaration named, so the response
// describing it says which URI, rather than referring to the shared `Problem`
// component and admitting every problem the service can produce. These pin the
// emitted shape; `docs/errors.md` states the rule.

/// Two failures answering with one status, one naming its own type and one
/// taking the prefix the enum declares. A status several variants share has to
/// publish every type reachable through it.
#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum LookupError {
    #[error("no user with that id")]
    #[problem(
        status = 404,
        type = "https://errors.example.com/user-unknown",
        title = "User unknown"
    )]
    UserUnknown,

    #[error("no tenant with that slug")]
    #[problem(status = 404, title = "Tenant unknown")]
    TenantUnknown,
}

/// Two failures answering with one status, neither naming a type and the enum
/// declaring no `base`, so both publish `about:blank`. The schema is one
/// branch — a `oneOf` repeating a `const` is satisfied by two at once — but
/// the status still has two things to say about itself.
#[derive(Debug, thiserror::Error, ApiError)]
enum ReadError {
    #[error("no file at that path")]
    #[problem(status = 404, title = "File not found")]
    FileNotFound,

    #[error("the source file is missing")]
    #[problem(status = 404, title = "Source file missing")]
    SourceFileMissing,
}

/// Neither `type` nor `base`, so `Problem::new` writes `about:blank` and the
/// description has exactly that to say.
#[derive(Debug, thiserror::Error, ApiError)]
#[error("the store is unavailable")]
#[problem(status = 503)]
struct StoreUnavailable;

/// The responses `T` declares, as JSON.
fn emitted_responses<T: Responses>() -> serde_json::Value {
    let mut registry = kynos::schema::registry::Registry::new();
    serde_json::to_value(T::responses(&mut registry)).expect("a set of responses serializes")
}

/// The problem schema one status declares, as JSON.
fn declared_problem(responses: &serde_json::Value, status: &str) -> serde_json::Value {
    responses[status]["content"]["application/problem+json"]["schema"].clone()
}

/// A status one variant answers with is the shared component *and* the type
/// that variant publishes, rather than the component alone.
#[test]
fn a_derived_error_response_narrows_to_the_type_it_publishes() {
    let responses = emitted_responses::<StoreError>();
    let schema = declared_problem(&responses, "409");
    let branches = schema["allOf"]
        .as_array()
        .unwrap_or_else(|| panic!("a narrowed problem response is an `allOf`: {schema}"));

    assert_eq!(branches.len(), 2, "{schema}");
    assert_eq!(
        branches[0]["$ref"],
        serde_json::json!("#/components/schemas/Problem")
    );
    assert_eq!(
        branches[1]["properties"]["type"],
        serde_json::json!({
            "type": "string",
            "const": "https://errors.example.com/email-taken",
        }),
        "{schema}"
    );
}

/// A status several variants share is a `oneOf` over the types they publish,
/// each branch naming itself.
#[test]
fn a_shared_status_publishes_every_type_that_reaches_it() {
    let responses = emitted_responses::<LookupError>();
    let schema = declared_problem(&responses, "404");
    let branches = schema["oneOf"]
        .as_array()
        .unwrap_or_else(|| panic!("a shared status is a `oneOf`: {schema}"));

    let published: Vec<&serde_json::Value> = branches
        .iter()
        .map(|branch| &branch["allOf"][1]["properties"]["type"]["const"])
        .collect();
    let titles: Vec<&serde_json::Value> = branches.iter().map(|branch| &branch["title"]).collect();

    assert_eq!(
        published,
        vec![
            &serde_json::json!("https://errors.example.com/user-unknown"),
            &serde_json::json!("https://errors.example.com/tenant-unknown"),
        ],
        "{schema}"
    );
    assert_eq!(
        titles,
        vec![
            &serde_json::json!("User unknown"),
            &serde_json::json!("Tenant unknown"),
        ],
        "{schema}"
    );
}

/// The description of a shared status names every variant answering with it,
/// not whichever one was declared first.
#[test]
fn a_shared_status_description_names_every_variant() {
    let responses = emitted_responses::<LookupError>();

    assert_eq!(
        responses["404"]["description"],
        serde_json::json!("User unknown; Tenant unknown"),
        "{responses}"
    );
}

/// A failure that names no type still narrows: `about:blank` is what the
/// serializer writes, so it is what the description can state.
#[test]
fn an_error_naming_no_type_narrows_to_about_blank() {
    let responses = emitted_responses::<StoreUnavailable>();
    let schema = declared_problem(&responses, "503");

    assert_eq!(
        schema["allOf"][1]["properties"]["type"]["const"],
        serde_json::json!("about:blank"),
        "{schema}"
    );
}

/// Untyped variants collapse to one schema branch, because they publish one
/// URI. The description is not a schema and does not collapse with it: every
/// variant answering with the status is named there.
#[test]
fn untyped_variants_sharing_a_status_are_each_named() {
    let responses = emitted_responses::<ReadError>();
    let schema = declared_problem(&responses, "404");

    assert_eq!(schema.get("oneOf"), None, "{schema}");
    assert_eq!(
        schema["allOf"][1]["properties"]["type"]["const"],
        serde_json::json!("about:blank"),
        "{schema}"
    );
    assert_eq!(
        responses["404"]["description"],
        serde_json::json!("File not found; Source file missing"),
        "{responses}"
    );
}
