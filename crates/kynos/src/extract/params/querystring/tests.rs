use serde_json::json;

use super::{admits_null, is_json};

/// Whether `schema`, written as JSON, admits `null`.
fn admits(schema: serde_json::Value) -> bool {
    admits_null(&serde_json::from_value(schema).expect("a schema"))
}

/// Every shape `Option<T>` describes itself as admits `null`; anything not
/// recognised does not, so the parameter errs towards `required`.
#[test]
fn null_is_admitted_only_where_the_schema_visibly_says_so() {
    assert!(admits(json!(true)));
    assert!(admits(json!({ "type": "null" })));
    assert!(admits(json!({ "type": ["string", "null"] })));
    assert!(admits(json!({
        "anyOf": [{ "$ref": "#/components/schemas/Filter" }, { "type": "null" }]
    })));
    assert!(admits(
        json!({ "oneOf": [{ "type": "integer" }, { "type": "null" }] })
    ));

    assert!(!admits(json!(false)));
    assert!(!admits(json!({ "type": "string" })));
    assert!(!admits(json!({ "$ref": "#/components/schemas/Filter" })));
    assert!(!admits(
        json!({ "anyOf": [{ "type": "integer" }, { "type": "string" }] })
    ));
    // A `const` pins one value, whatever `type` would otherwise allow.
    assert!(!admits(json!({ "type": ["string", "null"], "const": "x" })));
    // Unrecognised rather than refused: an unconstrained object schema.
    assert!(!admits(json!({})));
}

/// A structured syntax suffix is JSON, which is what lets a vendor media
/// type be decoded as the JSON it is.
#[test]
fn a_json_suffixed_media_type_is_read_as_json() {
    assert!(is_json("application/json"));
    assert!(is_json("application/json; charset=utf-8"));
    assert!(is_json("APPLICATION/JSON"));
    assert!(is_json("application/vnd.acme.filter+json"));

    assert!(!is_json("application/xml"));
    assert!(!is_json("text/plain"));
    // A suffix is a suffix of the base type, not of the parameters.
    assert!(!is_json("application/xml; note=+json"));
}
