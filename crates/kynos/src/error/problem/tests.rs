//! What one status declares.
//!
//! The shapes a declared status takes -- the `allOf` and the `oneOf` -- are
//! asserted through the derive in `crates/kynos/tests/derives.rs`, which is
//! where a reader meets them. What is left here is what no declaration reaches
//! as readably: two failures publishing one type, two sharing one summary, a
//! status whose failures gave no summary at all, and a status with no failure
//! answering it -- which no caller can emit and this refuses.

use serde_json::Value;

use crate::error::problem::narrowed_response;

/// The `Problem` component, as `Registry::resolve` hands it over.
fn component() -> kynos_openapi::Schema {
    kynos_openapi::Schema::component("Problem")
}

/// The schema one status declares, as JSON.
fn declared(status: u16, branches: &[(Option<&'static str>, Option<&'static str>)]) -> Value {
    let response = narrowed_response(&component(), status, branches);
    serde_json::to_value(response).expect("a response serializes")
}

/// A `oneOf` whose branches repeat a `const` is satisfied by two of them at
/// once, which is exactly what `oneOf` forbids. Two failures publishing one
/// type therefore declare one branch — while the description, which is prose
/// under no such rule, still names both.
#[test]
fn two_failures_publishing_one_type_declare_one_branch() {
    let response = declared(
        404,
        &[
            (Some("https://errors.example.com/unknown"), Some("Unknown")),
            (
                Some("https://errors.example.com/unknown"),
                Some("Also unknown"),
            ),
        ],
    );
    let schema = &response["content"]["application/problem+json"]["schema"];

    assert_eq!(schema.get("oneOf"), None, "{schema}");
    assert_eq!(
        schema["allOf"][1]["properties"]["type"]["const"],
        serde_json::json!("https://errors.example.com/unknown"),
        "{schema}"
    );
    assert_eq!(
        response["description"],
        serde_json::json!("Unknown; Also unknown")
    );
}

/// With no summary anywhere the description falls back to the status code's own
/// reason phrase, and to a sentence where the code has none.
#[test]
fn a_status_no_failure_summarized_describes_itself() {
    assert_eq!(
        declared(409, &[(Some("https://errors.example.com/taken"), None)])["description"],
        serde_json::json!("Conflict")
    );
    assert_eq!(
        declared(599, &[(None, None)])["description"],
        serde_json::json!("the request failed")
    );
}

/// Two failures may be summarized identically -- one sentence covering a 404
/// raised for two different resources. The schema keeps a branch per type,
/// because the types differ; the description writes the sentence once, because
/// repeating it word for word tells a reader nothing.
#[test]
fn one_summary_two_failures_share_is_written_once() {
    let response = declared(
        404,
        &[
            (
                Some("https://errors.example.com/user-unknown"),
                Some("Not found"),
            ),
            (
                Some("https://errors.example.com/tenant-unknown"),
                Some("Not found"),
            ),
        ],
    );
    let schema = &response["content"]["application/problem+json"]["schema"];

    assert_eq!(
        schema["oneOf"].as_array().map(Vec::len),
        Some(2),
        "{schema}"
    );
    assert_eq!(response["description"], serde_json::json!("Not found"));
}

/// A status with no failure answering it is not a narrowing of anything, and
/// neither caller can produce one. A caller that does is told, rather than
/// handed the unnarrowed component and left to read it as a narrowed one.
#[test]
#[should_panic(expected = "no caller passes an empty branch list")]
fn a_status_no_failure_answers_is_refused() {
    let _ = declared(404, &[]);
}

/// The `oneOf` is sound only because `Problem` requires `type`.
///
/// Each branch is `allOf: [$ref Problem, {properties: {type: {const: …}}}]`,
/// and `properties` says nothing about a member being *present*. A body
/// omitting `type` would therefore satisfy the `properties` half of every
/// branch at once, and `oneOf` would refuse a document `Problem` itself calls
/// valid. What stops that is the `$ref` half: `type` is in `Problem`'s
/// `required`, so an absent one fails every branch instead of matching them
/// all.
///
/// That coupling is between two files and is what the narrowing rests on, so
/// it is asserted rather than left to be rediscovered by whoever removes the
/// entry.
#[test]
fn the_one_of_rests_on_problem_requiring_its_type() {
    use crate::{error::problem::Problem, schema::Schema};

    let mut registry = crate::schema::registry::Registry::new();
    let schema = <Problem as Schema>::schema(&mut registry);
    let json = serde_json::to_value(&schema).expect("a schema serializes");

    let required = json
        .get("required")
        .and_then(Value::as_array)
        .expect("Problem declares required members");

    assert!(
        required.iter().any(|member| member == "type"),
        "`type` left `Problem`'s required members, so a body omitting it now \
         matches every `oneOf` branch at once and the narrowing refuses a \
         document Problem calls valid"
    );
}
