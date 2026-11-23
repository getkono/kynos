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
