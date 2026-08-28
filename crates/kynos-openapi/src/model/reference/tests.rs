use super::{Ref, RefOr};

#[test]
fn component_reference_helpers_build_json_pointers() {
    assert_eq!(Ref::schema("User").location, "#/components/schemas/User");
    assert_eq!(
        Ref::security_scheme("Bearer").location,
        "#/components/securitySchemes/Bearer"
    );
}

#[test]
fn an_object_carrying_ref_deserializes_as_a_reference() {
    let value: RefOr<crate::Example> =
        serde_json::from_str(r##"{"$ref": "#/components/examples/One"}"##).expect("valid");
    assert!(value.is_ref());
}

#[test]
fn reference_omits_absent_overrides() {
    let json = serde_json::to_string(&Ref::schema("User")).expect("serializable");
    assert_eq!(json, r##"{"$ref":"#/components/schemas/User"}"##);
}
