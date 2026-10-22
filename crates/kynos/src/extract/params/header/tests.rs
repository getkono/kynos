use kynos_openapi::{Example, MediaType, Parameter, ParameterIn, Ref, Schema};
use serde_json::json;

use super::{HeaderParams, Headers, header_from};
use crate::{
    extract::describe::Describe, router::operation::OperationCx, schema::registry::Registry,
};

/// A group that is described, as every group is by default.
struct Traced;

impl HeaderParams for Traced {
    const NAMES: &'static [&'static str] = &["x-trace"];
}

/// The same group, opted out of the description.
struct Untraced;

impl HeaderParams for Untraced {
    const NAMES: &'static [&'static str] = &["x-trace"];
    const DESCRIBED: bool = false;
}

/// The header parameter names `Headers<T>` adds to an operation.
fn described<T: HeaderParams>() -> Vec<String> {
    let mut registry = Registry::new();
    let mut operation = OperationCx::new(&mut registry);
    Headers::<T>::describe(&mut operation);

    operation
        .finish()
        .parameters
        .into_iter()
        .map(|parameter| match parameter {
            kynos_openapi::RefOr::Item(item) => item.name,
            kynos_openapi::RefOr::Ref(reference) => {
                panic!("an inline parameter, not {reference:?}")
            }
        })
        .collect()
}

/// `DESCRIBED` decides whether a group's parameters reach the operation.
#[test]
fn only_a_described_group_adds_its_parameters() {
    assert_eq!(described::<Traced>(), ["x-trace"]);
    assert!(described::<Untraced>().is_empty());
}

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
