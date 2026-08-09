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
