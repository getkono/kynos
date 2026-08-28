//! Emitted documents, checked against the OAI's own schema for a document.
//!
//! Every other conformance test in this workspace is Kynos checking Kynos:
//! `conformance.rs` checks a running service against the description it
//! emitted, `conformance_corpus.rs` checks that the committed corpus is what
//! this build emits, and `kynos-openapi`'s own rules check a document against
//! what those rules say. All three agree with the implementation by
//! construction, including where the implementation is wrong.
//!
//! This one does not. The two files it reads are the OAI's, vendored in
//! [`references/`](../../../references/README.md), and they are the only
//! statement in this repository about what an OpenAPI document is that Kynos
//! did not write. It closes the `docs/nfr.md` row that read "Emitted documents
//! validate against both 3.1 and 3.2 validators".
//!
//! # What this cannot catch
//!
//! Both are the *base* schemas, which describe the OpenAPI structure and leave
//! the Schema Object open, because 3.1 permits arbitrary keywords there. A
//! 3.2-only keyword nested inside a schema -- `xml.nodeType`,
//! `discriminator.defaultMapping` -- is therefore not something the 3.1 file
//! objects to. `kynos_openapi::emit::downgrade` is what refuses those, and the
//! two checks are kept because neither subsumes the other.

// `test-util` carries the JSON Schema validator this needs, and is in the list
// rather than in a `required-features` because there is no `[[test]]` to put
// one on: the target is auto-discovered so that `crates/kynos/Cargo.toml` can
// keep the file out of the published archive, where its oracle does not exist.
#![cfg(all(
    feature = "macros",
    feature = "json",
    feature = "openapi32",
    feature = "test-util"
))]

use std::path::PathBuf;

use kynos::{
    Router,
    openapi::{Document, SpecVersion},
    prelude::*,
};
use serde::{Deserialize, Serialize};

/// A user of the service.
#[derive(Schema, Serialize, Deserialize)]
struct User {
    /// The user's identifier.
    id: u64,
    /// The user's display name.
    name: String,
}

/// What `/users/{id}` captures.
#[allow(dead_code)]
#[derive(Schema, PathParams)]
struct UserPath {
    /// The identifier from the path.
    id: u64,
}

/// How a listing is paged.
#[allow(dead_code)]
#[derive(Schema, QueryParams)]
struct Page {
    /// Which page to return, counting from one.
    page: u32,
}

/// What creating a user can fail with.
#[allow(dead_code)]
#[derive(Debug, thiserror::Error, ApiError)]
#[problem(base = "https://errors.example.com/")]
enum StoreError {
    /// That name is already taken.
    #[error("that name is already taken")]
    #[problem(status = 409, type = "https://errors.example.com/name-taken")]
    NameTaken,
}

/// Lists users.
#[kynos::get("/users")]
async fn list_users(Query(page): Query<Page>) -> Json<Vec<User>> {
    let _ = page;
    Json(Vec::new())
}

/// Fetches one user.
#[kynos::get("/users/{id}")]
async fn get_user(Path(path): Path<UserPath>) -> Json<User> {
    Json(User {
        id: path.id,
        name: "Ada Lovelace".to_owned(),
    })
}

/// Creates a user.
#[kynos::post("/users")]
async fn create_user(Json(user): Json<User>) -> Result<Created<Json<User>>, StoreError> {
    Ok(Created::at(
        get_user::relative_uri(UserPath { id: user.id }),
        Json(user),
    ))
}

/// Reports that the service is up.
#[kynos::get("/health")]
async fn health() -> kynos::response::status::NoContent {
    kynos::response::status::NoContent
}

fn fixture() -> Router<()> {
    Router::<()>::new().mount(kynos::routes![health, list_users, get_user, create_user])
}

/// One vendored meta-schema, by file name.
fn meta_schema(file: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../references")
        .join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is vendored: {error}", path.display()));
    serde_json::from_str(&text).expect("a vendored meta-schema is JSON")
}

/// Checks `document` against `file`, returning every complaint.
fn complaints(document: &Document, file: &str) -> Vec<String> {
    let schema = meta_schema(file);
    let instance = serde_json::to_value(document).expect("an emitted document is serializable");

    let validator = jsonschema::draft202012::new(&schema)
        .unwrap_or_else(|error| panic!("{file} compiles as draft 2020-12: {error}"));

    validator
        .iter_errors(&instance)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect()
}

#[test]
fn a_three_one_document_satisfies_the_three_one_schema() {
    let document = fixture()
        .openapi_as(SpecVersion::V3_1)
        .expect("the fixture uses no 3.2-only construct");

    assert_eq!(document.openapi, "3.1.2");
    let complaints = complaints(&document, "oas-3.1-schema-2022-10-07.json");
    assert!(
        complaints.is_empty(),
        "emitted 3.1 document: {complaints:#?}"
    );
}

#[test]
fn a_three_two_document_satisfies_the_three_two_schema() {
    let document = fixture()
        .openapi_as(SpecVersion::V3_2)
        .expect("3.2 expresses everything 3.1 does");

    assert_eq!(document.openapi, "3.2.0");
    let complaints = complaints(&document, "oas-3.2-schema-2025-09-17.json");
    assert!(
        complaints.is_empty(),
        "emitted 3.2 document: {complaints:#?}"
    );
}

/// The corpus a downstream generator is built against is a valid 3.2 document.
///
/// `conformance_corpus.rs` asserts the committed file is what this build
/// emits; that says nothing about whether either is legal. This reads the same
/// committed text, because that file is what the other repository consumes.
#[test]
fn the_committed_corpus_satisfies_the_three_two_schema() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/conformance/sequential-3.2.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is committed: {error}", path.display()));
    let instance: serde_json::Value =
        serde_json::from_str(&text).expect("the committed corpus is JSON");

    let schema = meta_schema("oas-3.2-schema-2025-09-17.json");
    let validator =
        jsonschema::draft202012::new(&schema).expect("the 3.2 schema compiles as draft 2020-12");

    let complaints: Vec<String> = validator
        .iter_errors(&instance)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    assert!(complaints.is_empty(), "committed corpus: {complaints:#?}");
}

/// A document carrying a 3.2 construct is refused as 3.1 rather than emitted.
///
/// The guarantee `emit` states, checked here against the schema that would have
/// rejected the result: if the refusal ever stopped working, the document it
/// let through would fail the 3.1 file above.
#[test]
fn a_three_two_construct_cannot_be_emitted_as_three_one() {
    let mut document = fixture()
        .openapi_as(SpecVersion::V3_2)
        .expect("3.2 expresses everything 3.1 does");
    document.self_uri = Some("https://example.com/users".to_owned());

    let error = document
        .emit(SpecVersion::V3_1)
        .expect_err("`$self` is 3.2-only");
    assert!(
        format!("{error}").contains("$self"),
        "the refusal names what stands in the way: {error}"
    );
}

/// The control for the three assertions above.
///
/// A check that nothing fails is worth what a check that *cannot* fail is
/// worth, so this feeds the 3.1 file a document 3.1 does not allow and asserts
/// it complains. `$self` is the clearest case: a root field 3.2 introduced,
/// which the 3.1 schema closes the root against.
#[test]
fn the_three_one_schema_refuses_a_document_three_one_does_not_allow() {
    let document = fixture()
        .openapi_as(SpecVersion::V3_1)
        .expect("the fixture uses no 3.2-only construct");

    let mut instance = serde_json::to_value(&document).expect("serializable");
    instance
        .as_object_mut()
        .expect("a document is an object")
        .insert(
            "$self".to_owned(),
            serde_json::Value::String("https://example.com/users".to_owned()),
        );

    let schema = meta_schema("oas-3.1-schema-2022-10-07.json");
    let validator =
        jsonschema::draft202012::new(&schema).expect("the 3.1 schema compiles as draft 2020-12");

    assert!(
        validator.iter_errors(&instance).next().is_some(),
        "the 3.1 schema must reject a root field only 3.2 defines"
    );
}
