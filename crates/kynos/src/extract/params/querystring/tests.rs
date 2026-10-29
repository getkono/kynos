use serde_json::json;

use super::admits_null;

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
    // So may `enum`, `allOf` and `not`, which only ever narrow `type`: none is
    // evaluated, so each is unrecognised.
    assert!(!admits(
        json!({ "type": ["string", "null"], "enum": ["a"] })
    ));
    assert!(!admits(json!({
        "type": ["string", "null"],
        "allOf": [{ "type": "string" }]
    })));
    assert!(!admits(json!({
        "type": ["string", "null"],
        "not": { "type": "null" }
    })));
    // Unrecognised rather than refused: an unconstrained object schema.
    assert!(!admits(json!({})));
}

/// A media type marker written as the docs write it, deriving nothing.
struct Bare;

/// The marker sits only in a `PhantomData`, so no trait may be bounded on it:
/// each holds wherever the value's does, and compares the value alone.
#[test]
fn a_marker_deriving_nothing_leaves_query_string_its_traits() {
    use std::cmp::Ordering;

    use super::QueryString;

    let query = QueryString::<u64, Bare>::new(7);
    let copied = query;
    assert_eq!(query, copied, "a `Copy` value is still usable once moved");
    assert_eq!(query.clone(), query);
    assert_ne!(query, QueryString::<u64, Bare>::new(8));
    assert_eq!(
        QueryString::<u64, Bare>::default(),
        QueryString::<u64, Bare>::new(0)
    );
    assert_eq!(format!("{query:?}"), "QueryString { value: 7, .. }");
    assert_eq!(query.cmp(&QueryString::new(8)), Ordering::Less);
    assert_eq!(
        query.partial_cmp(&QueryString::new(6)),
        Some(Ordering::Greater)
    );
    let set: std::collections::HashSet<_> = [query, copied, QueryString::new(8)].into();
    assert_eq!(set.len(), 2, "equal values hash alike");
}
