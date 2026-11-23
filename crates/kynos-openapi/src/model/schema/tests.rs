use crate::model::schema::{
    Schema,
    dialect::OAS_DIALECT,
    types::{SchemaType, TypeSet},
};

#[test]
fn the_dialect_is_shared_between_three_one_and_three_two() {
    assert_eq!(
        OAS_DIALECT,
        "https://spec.openapis.org/oas/3.1/dialect/base"
    );
}

#[test]
fn boolean_schemas_serialize_as_bare_booleans() {
    assert_eq!(serde_json::to_string(&Schema::any()).expect("ok"), "true");
    assert_eq!(
        serde_json::to_string(&Schema::never()).expect("ok"),
        "false"
    );
}

#[test]
fn nullability_is_a_type_union_never_the_three_zero_nullable_keyword() {
    let schema = Schema::nullable(SchemaType::String);
    let json = serde_json::to_string(&schema).expect("ok");
    assert_eq!(json, r#"{"type":["string","null"]}"#);
    assert!(!json.contains("nullable"));
}

#[test]
fn a_single_type_serializes_unwrapped() {
    let schema = Schema::of_type(SchemaType::Integer);
    assert_eq!(
        serde_json::to_string(&schema).expect("ok"),
        r#"{"type":"integer"}"#
    );
}

#[test]
fn type_sets_round_trip() {
    let one: TypeSet = serde_json::from_str(r#""string""#).expect("ok");
    assert_eq!(one, TypeSet::One(SchemaType::String));

    let many: TypeSet = serde_json::from_str(r#"["string","null"]"#).expect("ok");
    assert_eq!(
        many,
        TypeSet::Many(vec![SchemaType::String, SchemaType::Null])
    );
}

#[test]
fn component_references_use_the_schema_ref_keyword() {
    let json = serde_json::to_string(&Schema::component("User")).expect("ok");
    assert_eq!(json, r##"{"$ref":"#/components/schemas/User"}"##);
}

/// A described boolean schema still accepts exactly what the boolean did, and
/// an object keeps its other keywords.
#[test]
fn a_description_promotes_a_boolean_schema_without_changing_what_it_accepts() {
    let described =
        |schema: Schema| serde_json::to_string(&schema.with_description("d")).expect("ok");
    assert_eq!(described(Schema::any()), r#"{"description":"d"}"#);
    assert_eq!(
        described(Schema::never()),
        r#"{"not":true,"description":"d"}"#
    );
    assert_eq!(
        described(Schema::of_type(SchemaType::String)),
        r#"{"type":"string","description":"d"}"#
    );
}

/// Editing in place never promotes a boolean schema.
#[test]
fn a_boolean_schema_has_no_keywords_to_edit() {
    assert!(Schema::any().as_object_mut().is_none());
    assert!(Schema::never().as_object_mut().is_none());
    assert!(Schema::default().as_object_mut().is_some());
}

fn parsed(json: serde_json::Value) -> Schema {
    serde_json::from_value(json).expect("schema")
}

/// Each pointer `schemas` yields, with the `description` at it.
fn described<'s>(
    schemas: impl Iterator<Item = (String, &'s Schema)>,
) -> Vec<(String, Option<&'s str>)> {
    schemas
        .map(|(pointer, schema)| {
            let description = schema
                .as_object()
                .and_then(|object| object.description.as_deref());
            (pointer, description)
        })
        .collect()
}

#[test]
fn subschemas_are_the_direct_ones_at_relative_escaped_pointers_in_keyword_order() {
    let schema = parsed(serde_json::json!({
        "properties": {
            "a/b~c": { "description": "property", "items": { "description": "deeper" } }
        },
        "items": { "description": "items" },
        "oneOf": [true, { "description": "second" }],
        "$defs": { "Inner": { "description": "def" } }
    }));

    let object = schema.as_object().expect("keywords");
    assert_eq!(
        described(object.subschemas()),
        [
            ("/$defs/Inner".to_owned(), Some("def")),
            ("/oneOf/0".to_owned(), None),
            ("/oneOf/1".to_owned(), Some("second")),
            ("/items".to_owned(), Some("items")),
            ("/properties/a~1b~0c".to_owned(), Some("property")),
        ]
    );
}

#[test]
fn a_walk_is_depth_first_each_schema_before_what_it_holds_and_stops_at_a_ref() {
    let schema = parsed(serde_json::json!({
        "description": "root",
        "allOf": [
            { "description": "first", "not": { "description": "below first" } },
            { "$ref": "#/components/schemas/Other", "description": "a ref" }
        ],
        "additionalProperties": false
    }));

    assert_eq!(
        described(schema.walk()),
        [
            (String::new(), Some("root")),
            ("/allOf/0".to_owned(), Some("first")),
            ("/allOf/0/not".to_owned(), Some("below first")),
            ("/allOf/1".to_owned(), Some("a ref")),
            ("/additionalProperties".to_owned(), None),
        ]
    );
}

#[test]
fn a_boolean_schema_walks_to_itself_alone() {
    let schema = Schema::never();
    let walked: Vec<_> = schema.walk().collect();
    assert_eq!(walked, [(String::new(), &schema)]);
}
