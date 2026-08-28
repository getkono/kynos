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

// `every_keyword` pins sixty keys in one `json!`, and the macro recurses once
// per key.
#![recursion_limit = "256"]

use std::collections::BTreeSet;

use kynos_openapi::{
    Callback, ComponentName, Components, Contact, Discriminator, Document, Encoding, EncodingStyle,
    Example, Extensions, ExternalDocumentation, Header, HeaderStyle, Info, License, Link, Map,
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
        // Every keyword at once rather than a representative one. See
        // [`every_keyword`] for why one is not enough.
        {
            let (object, expected) = every_keyword();
            case("SchemaObject", &object, expected)
        },
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
                "info": { "title": "Orders", "version": "1.0.0" },
                // Always present, even empty: a document carrying none of
                // `paths`, `components` or `webhooks` violates a MUST in every
                // version, and this is the one of the three that is always
                // true of an API.
                "paths": {}
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

// --- Every schema keyword ------------------------------------------------

/// A `SchemaObject` with every keyword set, beside the exact JSON it emits.
///
/// `SchemaObject` carries sixty keywords and the case in `schema_cases`
/// pins one of them. The other fifty-nine were spelled by a `serde(rename)`
/// nothing read: `unknown_keywords` is `#[serde(flatten)]`, so a misspelled
/// rename is written under the wrong key, read back into the flattened map,
/// and compares equal through the round-trip property. The keyword simply
/// stops existing, and every test in the crate still passes.
///
/// Each value is distinguishable from its neighbours so that two renames
/// swapped between adjacent fields do not cancel out.
fn every_keyword() -> (SchemaObject, Value) {
    (every_keyword_set(), every_keyword_emitted())
}

/// The value half of [`every_keyword`].
#[expect(
    deprecated,
    reason = "`example` is superseded by `examples` and still emits, so its spelling is still \
              part of the wire shape; a deprecated keyword nobody pins is one that can be \
              renamed silently"
)]
fn every_keyword_set() -> SchemaObject {
    let one = || [("Node".to_owned(), Schema::any())].into_iter().collect();

    SchemaObject {
        schema_dialect: Some("https://json-schema.org/draft/2020-12/schema".to_owned()),
        id: Some("https://example.com/order.json".to_owned()),
        reference: Some("#/components/schemas/Order".to_owned()),
        anchor: Some("order".to_owned()),
        dynamic_ref: Some("#node".to_owned()),
        dynamic_anchor: Some("node".to_owned()),
        comment: Some("not part of the description".to_owned()),
        defs: one(),
        all_of: Some(vec![Schema::any()]),
        any_of: Some(vec![Schema::any(), Schema::never()]),
        one_of: Some(vec![Schema::never()]),
        not: Some(Box::new(Schema::never())),
        if_schema: Some(Box::new(Schema::any())),
        then_schema: Some(Box::new(Schema::never())),
        else_schema: Some(Box::new(Schema::any())),
        dependent_schemas: one(),
        prefix_items: Some(vec![Schema::never()]),
        items: Some(Box::new(Schema::any())),
        contains: Some(Box::new(Schema::never())),
        properties: one(),
        pattern_properties: one(),
        additional_properties: Some(Box::new(Schema::never())),
        property_names: Some(Box::new(Schema::any())),
        unevaluated_items: Some(Box::new(Schema::never())),
        unevaluated_properties: Some(Box::new(Schema::any())),
        ty: Some(TypeSet::One(SchemaType::Object)),
        const_value: Some(json!("fixed")),
        enumeration: Some(vec![json!("a"), json!("b")]),
        multiple_of: Some(2.5),
        maximum: Some(10.5),
        exclusive_maximum: Some(11.5),
        minimum: Some(1.5),
        exclusive_minimum: Some(0.5),
        max_length: Some(64),
        min_length: Some(1),
        pattern: Some("^order-".to_owned()),
        max_items: Some(9),
        min_items: Some(2),
        unique_items: Some(true),
        max_contains: Some(4),
        min_contains: Some(3),
        max_properties: Some(6),
        min_properties: Some(5),
        required: Some(vec!["id".to_owned()]),
        dependent_required: [("id".to_owned(), vec!["revision".to_owned()])]
            .into_iter()
            .collect(),
        format: Some("uuid".to_owned()),
        content_encoding: Some("base64".to_owned()),
        content_media_type: Some("application/json".to_owned()),
        content_schema: Some(Box::new(Schema::any())),
        title: Some("Order".to_owned()),
        description: Some("A placed order.".to_owned()),
        default: Some(json!({ "id": "none" })),
        deprecated: Some(true),
        read_only: Some(true),
        write_only: Some(false),
        examples: Some(vec![json!("order-1")]),
        discriminator: Some(Discriminator::new("petType")),
        xml: Some(Xml {
            name: Some("order".to_owned()),
            ..Xml::default()
        }),
        external_docs: Some(ExternalDocumentation::new("https://example.com/docs")),
        example: Some(json!("order-0")),
        // Left empty on purpose: it is `#[serde(flatten)]`, so anything here
        // would emit keys that are not keywords and the count below would stop
        // meaning what it says.
        unknown_keywords: Map::default(),
    }
}

/// The JSON half of [`every_keyword`].
fn every_keyword_emitted() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://example.com/order.json",
        "$ref": "#/components/schemas/Order",
        "$anchor": "order",
        "$dynamicRef": "#node",
        "$dynamicAnchor": "node",
        "$comment": "not part of the description",
        "$defs": { "Node": true },
        "allOf": [true],
        "anyOf": [true, false],
        "oneOf": [false],
        "not": false,
        "if": true,
        "then": false,
        "else": true,
        "dependentSchemas": { "Node": true },
        "prefixItems": [false],
        "items": true,
        "contains": false,
        "properties": { "Node": true },
        "patternProperties": { "Node": true },
        "additionalProperties": false,
        "propertyNames": true,
        "unevaluatedItems": false,
        "unevaluatedProperties": true,
        "type": "object",
        "const": "fixed",
        "enum": ["a", "b"],
        "multipleOf": 2.5,
        "maximum": 10.5,
        "exclusiveMaximum": 11.5,
        "minimum": 1.5,
        "exclusiveMinimum": 0.5,
        "maxLength": 64,
        "minLength": 1,
        "pattern": "^order-",
        "maxItems": 9,
        "minItems": 2,
        "uniqueItems": true,
        "maxContains": 4,
        "minContains": 3,
        "maxProperties": 6,
        "minProperties": 5,
        "required": ["id"],
        "dependentRequired": { "id": ["revision"] },
        "format": "uuid",
        "contentEncoding": "base64",
        "contentMediaType": "application/json",
        "contentSchema": true,
        "title": "Order",
        "description": "A placed order.",
        "default": { "id": "none" },
        "deprecated": true,
        "readOnly": true,
        "writeOnly": false,
        "examples": ["order-1"],
        "discriminator": { "propertyName": "petType" },
        "xml": { "name": "order" },
        "externalDocs": { "url": "https://example.com/docs" },
        "example": "order-0",
    })
}

/// The pinned keys, counted against the fields the struct declares.
///
/// Without this, a keyword added to `SchemaObject` is a keyword with no pinned
/// spelling — which is the state this test was written to end.
#[test]
fn every_schema_keyword_has_a_pinned_spelling() {
    const SOURCE: &str = include_str!("../src/model/schema/object.rs");

    let body = SOURCE
        .split_once("pub struct SchemaObject {")
        .expect("the struct is declared in this file")
        .1;
    // One `pub` field per keyword, less the flattened catch-all, which is not
    // a keyword and emits no key of its own.
    let declared = body
        .lines()
        .filter(|line| line.trim_start().starts_with("pub "))
        .count()
        - 1;

    let (_, expected) = every_keyword();
    let pinned = expected.as_object().expect("a JSON object").len();

    assert_eq!(
        pinned, declared,
        "`SchemaObject` declares {declared} keyword(s) and {pinned} have a pinned spelling; a \
         keyword added without one is written under whatever name its rename says, absorbed by \
         the flattened `unknown_keywords` on the way back, and round-trips perfectly while \
         meaning nothing"
    );
}
