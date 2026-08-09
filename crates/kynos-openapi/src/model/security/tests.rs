use crate::model::security::{SecurityScheme, requirement::SecurityRequirement};

#[test]
fn the_scheme_type_is_the_serde_tag() {
    let json = serde_json::to_string(&SecurityScheme::bearer(Some("JWT".to_owned())))
        .expect("serializable");
    assert!(json.contains(r#""type":"http""#));
    assert!(json.contains(r#""scheme":"bearer""#));
    assert!(json.contains(r#""bearerFormat":"JWT""#));
}

#[test]
fn mutual_tls_needs_no_further_configuration() {
    let json = serde_json::to_string(&SecurityScheme::mutual_tls()).expect("serializable");
    assert_eq!(json, r#"{"type":"mutualTLS"}"#);
}

#[test]
fn an_empty_requirement_means_anonymous_access() {
    assert!(SecurityRequirement::anonymous().is_anonymous());
    assert!(!SecurityRequirement::scheme("Bearer").is_anonymous());
}

#[test]
fn requirements_serialize_as_a_bare_map() {
    let json = serde_json::to_string(&SecurityRequirement::scoped("OAuth", ["read", "write"]))
        .expect("serializable");
    assert_eq!(json, r#"{"OAuth":["read","write"]}"#);
}
