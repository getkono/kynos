use kynos_openapi::{Example, MediaType, Parameter, ParameterIn, Ref, Schema};
use serde_json::json;

use super::header_from;

/// The header as it is written into a response's `headers` map.
fn written(parameter: &Parameter) -> serde_json::Value {
    serde_json::to_value(header_from(parameter)).expect("a serializable header")
}

/// A schema-shaped parameter keeps everything a Header Object can hold:
/// description, `required`, `deprecated` and an inline example. `name`, `in`
/// and the `simple` style, which a Header Object has no room for or only one
/// value of, are dropped.
#[test]
fn a_schema_parameter_becomes_the_header_of_the_same_value() {
    let mut parameter = Parameter::header("X-Rate-Limit", Schema::any())
        .with_description("requests left in the window")
        .required(true)
        .with_example(42);
    parameter.deprecated = Some(true);

    assert_eq!(
        written(&parameter),
        json!({
            "description": "requests left in the window",
            "required": true,
            "deprecated": true,
            "schema": true,
            "example": 42,
        })
    );
}

/// A content-shaped parameter becomes a content-shaped header, and named
/// examples travel whether each is inline or a `$ref`.
#[test]
fn a_content_parameter_becomes_a_content_header_with_its_named_examples() {
    let parameter = Parameter::with_content(
        "X-Trace",
        ParameterIn::Header,
        "application/json",
        MediaType::new(Schema::any()),
    )
    .with_named_example("inline", Example::new(json!({ "id": 1 })))
    .with_named_example_ref("shared", Ref::new("#/components/examples/Trace"));

    assert_eq!(
        written(&parameter),
        json!({
            "content": { "application/json": { "schema": true } },
            "examples": {
                "inline": { "value": { "id": 1 } },
                "shared": { "$ref": "#/components/examples/Trace" },
            },
        })
    );
}
