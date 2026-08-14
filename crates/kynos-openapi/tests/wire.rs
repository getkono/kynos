//! The exact JSON each model type emits.
//!
//! `properties.rs` proves that `parse ∘ emit` is the identity. That is not the
//! same guarantee: a misspelled field name round-trips perfectly, because
//! nothing in the model sets `deny_unknown_fields` and the flattened
//! `Extensions` absorbs whatever it does not recognise. A round trip proves
//! nothing was lost. These cases are what hold the shape to the specification.
//!
//! One case per type with a wire form, and
//! [`every_wire_type_has_a_case`] counts them against the source so a type
//! added without one fails the build.

use std::collections::BTreeSet;

use kynos_openapi::{
    Callback, ComponentName, Components, Contact, Discriminator, Document, Encoding, EncodingStyle,
    Example, Extensions, ExternalDocumentation, Header, HeaderStyle, Info, License, Link,
    MediaType, Method, OAuthFlow, OAuthFlows, Operation, Parameter, ParameterIn, PathItem,
    PathTemplate, Paths, Ref, RefOr, RequestBody, Response, Responses, Schema, SchemaObject,
    SecurityRequirement, SecurityScheme, Server, ServerVariable, SpecVersion, Style, Tag, Xml,
    model::schema::types::{SchemaType, TypeSet},
};
use serde::Serialize;
use serde_json::{Value, json};

/// A type, a value of it, and the JSON that value must produce.
struct Case {
    type_name: &'static str,
    emitted: Value,
    expected: Value,
}

fn case<T: Serialize>(type_name: &'static str, value: &T, expected: Value) -> Case {
    Case {
        type_name,
        emitted: serde_json::to_value(value).expect("every model value is representable"),
        expected,
    }
}

fn schema() -> Schema {
    Schema::of_type(SchemaType::String)
}

// --- Info, servers, tags -------------------------------------------------

fn metadata_cases() -> Vec<Case> {
    vec![
        case(
            "Info",
            &Info::new("Orders", "1.0.0"),
            json!({ "title": "Orders", "version": "1.0.0" }),
        ),
        case(
            "Contact",
            &Contact {
                name: Some("API Team".to_owned()),
                url: None,
                email: Some("api@example.com".to_owned()),
                extensions: Extensions::new(),
            },
            json!({ "name": "API Team", "email": "api@example.com" }),
        ),
        // The three shapes the specification allows, one of which is the whole
        // reason `License` holds a private enum rather than two `Option`s.
        case(
            "License",
            &License::spdx("MIT", "MIT"),
            json!({ "name": "MIT", "identifier": "MIT" }),
        ),
        case(
            "ExternalDocumentation",
            &ExternalDocumentation::new("https://example.com/docs"),
            json!({ "url": "https://example.com/docs" }),
        ),
        case(
            "Server",
            &Server::new("https://example.com/{region}").with_variable(
                "region",
                ServerVariable {
                    enumeration: Some(vec!["eu".to_owned(), "us".to_owned()]),
                    default_value: "eu".to_owned(),
                    description: None,
                    extensions: Extensions::new(),
                },
            ),
            json!({
                "url": "https://example.com/{region}",
                "variables": { "region": { "enum": ["eu", "us"], "default": "eu" } }
            }),
        ),
        case(
            "ServerVariable",
            &ServerVariable {
                enumeration: None,
                default_value: "eu".to_owned(),
                description: None,
                extensions: Extensions::new(),
            },
            json!({ "default": "eu" }),
        ),
        case("Tag", &Tag::new("orders"), json!({ "name": "orders" })),
        case(
            "Extensions",
            &{
                let mut extensions = Extensions::new();
                extensions.insert("x-internal-id", 7);
                extensions
            },
            json!({ "x-internal-id": 7 }),
        ),
    ]
}

// --- Schemas -------------------------------------------------------------

fn schema_cases() -> Vec<Case> {
    vec![
        // `Schema` is untagged over a bool and an object, so the boolean form
        // has to emit a bare `true` rather than a wrapper.
        case("Schema", &Schema::any(), json!(true)),
        case(
            "SchemaObject",
            &SchemaObject {
                max_length: Some(64),
                ..SchemaObject::default()
            },
            json!({ "maxLength": 64 }),
        ),
        case("SchemaType", &SchemaType::Integer, json!("integer")),
        // Untagged again: one type is a string, several are an array. This is
        // how 3.1 spells nullability, and why `nullable` is never emitted.
        case(
            "TypeSet",
            &TypeSet::One(SchemaType::String),
            json!("string"),
        ),
        case(
            "Discriminator",
            &Discriminator::new("petType"),
            json!({ "propertyName": "petType" }),
        ),
        case(
            "Xml",
            &Xml {
                name: Some("order".to_owned()),
                ..Xml::default()
            },
            json!({ "name": "order" }),
        ),
    ]
}

// --- References ----------------------------------------------------------

fn reference_cases() -> Vec<Case> {
    vec![
        case(
            "Ref",
            &Ref::schema("Order"),
            json!({ "$ref": "#/components/schemas/Order" }),
        ),
        // `RefOr` is untagged with `Ref` first, so a reference emits as the
        // bare Reference Object rather than as a tagged wrapper.
        case(
            "RefOr",
            &RefOr::<Schema>::Ref(Ref::schema("Order")),
            json!({ "$ref": "#/components/schemas/Order" }),
        ),
        case(
            "ComponentName",
            &ComponentName::new("Order").expect("valid"),
            json!("Order"),
        ),
    ]
}

// --- Parameters and headers ----------------------------------------------

fn parameter_cases() -> Vec<Case> {
    vec![
        case(
            "Parameter",
            &Parameter::path("id", schema()),
            json!({
                "name": "id",
                "in": "path",
                "required": true,
                "schema": { "type": "string" }
            }),
        ),
        case("ParameterIn", &ParameterIn::Query, json!("query")),
        case("Style", &Style::DeepObject, json!("deepObject")),
        case("HeaderStyle", &HeaderStyle::Simple, json!("simple")),
        case(
            "EncodingStyle",
            &EncodingStyle::SpaceDelimited,
            json!("spaceDelimited"),
        ),
        case(
            "Header",
            &Header::new(schema()),
            json!({ "schema": { "type": "string" } }),
        ),
    ]
}

// --- Bodies and content --------------------------------------------------

fn content_cases() -> Vec<Case> {
    vec![
        case(
            "MediaType",
            &MediaType::new(schema()),
            json!({ "schema": { "type": "string" } }),
        ),
        case(
            "Encoding",
            &Encoding::new("text/plain"),
            json!({ "contentType": "text/plain" }),
        ),
        case(
            "RequestBody",
            &RequestBody::json(schema()),
            json!({
                "content": { "application/json": { "schema": { "type": "string" } } },
                "required": true
            }),
        ),
        case(
            "Example",
            &Example::new(json!("sample")),
            json!({ "value": "sample" }),
        ),
    ]
}

// --- Responses -----------------------------------------------------------

fn response_cases() -> Vec<Case> {
    vec![
        case(
            "Response",
            &Response::new("ok"),
            json!({ "description": "ok" }),
        ),
        // `Responses` is a hand-written map: status keys, `default`, and `x-`
        // extensions, with nothing else accepted on the way back in.
        case(
            "Responses",
            &Responses::new().with(200, Response::new("ok")),
            json!({ "200": { "description": "ok" } }),
        ),
        case(
            "Link",
            &Link::to_operation("getOrder").with_parameter("orderId", json!("$response.body#/id")),
            json!({
                "operationId": "getOrder",
                "parameters": { "orderId": "$response.body#/id" }
            }),
        ),
    ]
}

// --- Paths and operations ------------------------------------------------

fn path_cases() -> Vec<Case> {
    let operation = || {
        Operation::new("getOrder").with_responses(Responses::new().with(200, Response::new("ok")))
    };

    vec![
        case("Method", &Method::Delete, json!("delete")),
        case(
            "PathTemplate",
            &PathTemplate::parse("/orders/{id}").expect("valid"),
            json!("/orders/{id}"),
        ),
        case(
            "Operation",
            &operation(),
            json!({
                "operationId": "getOrder",
                "responses": { "200": { "description": "ok" } }
            }),
        ),
        case(
            "PathItem",
            &PathItem::new().with_operation(Method::Get, operation()),
            json!({
                "get": {
                    "operationId": "getOrder",
                    "responses": { "200": { "description": "ok" } }
                }
            }),
        ),
        case(
            "Paths",
            &{
                let mut paths = Paths::new();
                paths.insert(
                    &PathTemplate::parse("/orders").expect("valid"),
                    PathItem::new().with_operation(Method::Get, operation()),
                );
                paths
            },
            json!({
                "/orders": {
                    "get": {
                        "operationId": "getOrder",
                        "responses": { "200": { "description": "ok" } }
                    }
                }
            }),
        ),
        case(
            "Callback",
            &Callback::new().with(
                "{$request.body#/callbackUrl}",
                PathItem::new().with_operation(Method::Post, operation()),
            ),
            json!({
                "{$request.body#/callbackUrl}": {
                    "post": {
                        "operationId": "getOrder",
                        "responses": { "200": { "description": "ok" } }
                    }
                }
            }),
        ),
    ]
}

// --- Security ------------------------------------------------------------

fn security_cases() -> Vec<Case> {
    vec![
        // Internally tagged on `type`, with the wire spellings the
        // specification gives rather than the Rust ones.
        case(
            "SecurityScheme",
            &SecurityScheme::api_key_header("X-Api-Key"),
            json!({ "type": "apiKey", "name": "X-Api-Key", "in": "header" }),
        ),
        case(
            "SecurityRequirement",
            &SecurityRequirement::scoped("OAuth", ["read:orders"]),
            json!({ "OAuth": ["read:orders"] }),
        ),
        case(
            "OAuthFlow",
            &OAuthFlow::new([("read:orders".to_owned(), "Read orders".to_owned())])
                .with_token_url("https://example.com/token"),
            json!({
                "tokenUrl": "https://example.com/token",
                "scopes": { "read:orders": "Read orders" }
            }),
        ),
        case(
            "OAuthFlows",
            &OAuthFlows {
                client_credentials: Some(
                    OAuthFlow::new([("read:orders".to_owned(), "Read orders".to_owned())])
                        .with_token_url("https://example.com/token"),
                ),
                ..OAuthFlows::default()
            },
            json!({
                "clientCredentials": {
                    "tokenUrl": "https://example.com/token",
                    "scopes": { "read:orders": "Read orders" }
                }
            }),
        ),
    ]
}

// --- Document and components ---------------------------------------------

fn document_cases() -> Vec<Case> {
    vec![
        case(
            "Components",
            &{
                let mut components = Components::new();
                components.insert_schema(&ComponentName::new("Order").expect("valid"), schema());
                components
            },
            json!({ "schemas": { "Order": { "type": "string" } } }),
        ),
        case(
            "Document",
            &Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0")),
            json!({
                "openapi": "3.1.2",
                "info": { "title": "Orders", "version": "1.0.0" }
            }),
        ),
    ]
}

fn cases() -> Vec<Case> {
    let mut cases = metadata_cases();
    cases.extend(schema_cases());
    cases.extend(reference_cases());
    cases.extend(parameter_cases());
    cases.extend(content_cases());
    cases.extend(response_cases());
    cases.extend(path_cases());
    cases.extend(security_cases());
    cases.extend(document_cases());
    cases
}

#[test]
fn each_type_emits_the_shape_the_specification_gives_it() {
    for Case {
        type_name,
        emitted,
        expected,
    } in cases()
    {
        assert_eq!(emitted, expected, "`{type_name}` emitted the wrong shape");
    }
}

/// Every model type with a wire form has a case above.
///
/// The set comes from the source rather than from a second list, so a type
/// added without a case fails here rather than going uncovered. `Serialize` is
/// the discriminator because it is what `docs/testing.md` uses to recognise the
/// kind: a type without it has no wire shape of its own to pin.
#[test]
fn every_wire_type_has_a_case() {
    let declared = wire_types_in_source();
    let covered: BTreeSet<String> = cases()
        .iter()
        .map(|case| case.type_name.to_owned())
        .collect();

    let uncovered: Vec<&String> = declared.difference(&covered).collect();
    let unknown: Vec<&String> = covered.difference(&declared).collect();

    assert!(
        uncovered.is_empty() && unknown.is_empty(),
        "types with no case: {uncovered:?}; cases naming no type: {unknown:?}"
    );
}

/// Types under `src/model/` that derive `Serialize`.
///
/// Reading the crate's own committed source keeps this hermetic — there is no
/// shared state and no ordering to depend on — and `CARGO_MANIFEST_DIR` keeps
/// it independent of where the suite is run from.
fn wire_types_in_source() -> BTreeSet<String> {
    fn walk(directory: &std::path::Path, found: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(directory).expect("the crate's own source is readable") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                walk(&path, found);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }

            let source = std::fs::read_to_string(&path).expect("readable source");
            let lines: Vec<&str> = source.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                let Some(rest) = line
                    .strip_prefix("pub struct ")
                    .or_else(|| line.strip_prefix("pub enum "))
                else {
                    continue;
                };
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();

                // The derive sits in the attribute block immediately above.
                let derives_serialize = lines[..index]
                    .iter()
                    .rev()
                    .take_while(|above| {
                        above.starts_with('#') || above.starts_with("//") || above.trim().is_empty()
                    })
                    .any(|above| above.contains("Serialize"));

                if derives_serialize {
                    found.insert(name);
                }
            }
        }
    }

    let mut found = BTreeSet::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/model"),
        &mut found,
    );
    found
}

// --- The two round-trip gaps `nfr.md` records ----------------------------
//
// Both are excluded from the generators in `support/`, which keeps the
// round-trip property honest but leaves the behaviour unrecorded. These are
// the record. Each asserts what happens *today*, so closing either gap turns a
// test red on purpose rather than passing in silence.

/// A JSON `null` example does not survive a round trip.
///
/// The loss is on the way *in*, not the way out: `Some(Value::Null)` writes
/// `null` faithfully, and `Option<Value>` then folds that `null` back into
/// `None` when it is read. JSON `null` is a legal example and a legal default,
/// so a description using one is silently changed. The remedy is a
/// double-`Option` deserializer at each site; when it lands, this test fails
/// and is replaced by its opposite.
///
/// `SchemaObject`'s `default` and `const` lose a `null` the same way, for the
/// same reason. One case records the shape of the gap; eight would not record
/// more of it.
#[test]
fn a_null_example_does_not_survive_a_round_trip() {
    let original = Example::new(Value::Null);
    let json = serde_json::to_string(&original).expect("serializable");
    let parsed: Example = serde_json::from_str(&json).expect("what the model emits, it reads");

    assert_eq!(json, r#"{"value":null}"#, "the null is written out");
    assert!(parsed.value().is_none(), "but does not come back");
    assert_ne!(parsed, original);
}

/// A `PathItem` carrying both `$ref` and siblings loses the siblings.
///
/// `RefOr` is untagged with `Ref` first, so anything carrying `$ref`
/// deserializes as a reference and its sibling fields are dropped. Kynos never
/// emits one, but the type permits constructing it, so the model can hold a
/// value it cannot write down.
#[test]
fn a_path_item_with_a_ref_and_siblings_loses_its_siblings() {
    let mut item = PathItem::new().with_operation(
        Method::Get,
        Operation::new("getOrder").with_responses(Responses::new().with(200, Response::new("ok"))),
    );
    item.reference = Some("#/components/pathItems/Shared".to_owned());

    let original = RefOr::Item(item);
    let json = serde_json::to_string(&original).expect("serializable");
    let parsed: RefOr<PathItem> = serde_json::from_str(&json).expect("reads back");

    assert!(
        parsed.is_ref(),
        "the item read back as a reference, not as itself"
    );
    assert_ne!(parsed, original, "so the operation did not survive");
}
