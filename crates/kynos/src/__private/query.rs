//! What a `#[derive(QueryParams)]` group's `parameters` does beyond building
//! each Parameter Object.

use std::fmt::Display;

use kynos_openapi::{Schema as OpenApiSchema, SchemaType, TypeSet};

use crate::{
    router::error::RouterError,
    schema::{constraints::Violations, registry::Registry},
};

/// `schema` stating `value` as its `default`, and `violations`, what checking
/// `value` against the field's bounds found, recorded on `registry` as a
/// [`ParameterDefault`](RouterError::ParameterDefault) naming `group`'s
/// parameter `name` where it is not empty.
pub fn default<T: Display>(
    registry: &mut Registry,
    group: &'static str,
    name: &str,
    schema: OpenApiSchema,
    value: &T,
    violations: Violations,
) -> OpenApiSchema {
    let text = value.to_string();
    if !violations.is_empty() {
        registry.refuse_parameter(RouterError::ParameterDefault {
            group,
            parameter: name.to_owned(),
            default: text.clone(),
            failures: violations.into_failures().into_values().collect(),
        });
    }
    with_default(schema, text)
}

/// Records on `registry`, as a
/// [`DuplicateQueryParameter`](RouterError::DuplicateQueryParameter) naming
/// `group`, each name of `parameters` that two of its members describe:
/// `members` holds each one's range, a member being one of `group`'s own
/// fields or one group it flattens. A name repeated inside one flattened
/// group is that group's to report, from its own `parameters`.
pub fn refuse_repeated(
    registry: &mut Registry,
    group: &'static str,
    parameters: &[kynos_openapi::Parameter],
    members: &[std::ops::Range<usize>],
) {
    for (index, member) in members.iter().enumerate() {
        for parameter in &parameters[member.clone()] {
            let earlier = members[..index].iter().any(|earlier| {
                parameters[earlier.clone()]
                    .iter()
                    .any(|seen| seen.name == parameter.name)
            });
            if earlier {
                registry.refuse_parameter(RouterError::DuplicateQueryParameter {
                    group,
                    parameter: parameter.name.clone(),
                });
            }
        }
    }
}

/// `schema` with `text`, what the parameter's value writes, as its `default`,
/// typed as `schema` types the value: a JSON number where it admits a number,
/// a boolean where it admits one, and the string otherwise, as a `$ref` to a
/// named string type is.
pub(crate) fn with_default(schema: OpenApiSchema, text: String) -> OpenApiSchema {
    // `true` is the empty keyword set, so promoting it loses nothing.
    let mut object = match schema {
        OpenApiSchema::Object(object) => object,
        OpenApiSchema::Bool(_) => Box::default(),
    };
    let admits = |wanted: SchemaType| match &object.ty {
        Some(TypeSet::One(ty)) => *ty == wanted,
        Some(TypeSet::Many(types)) => types.contains(&wanted),
        None => false,
    };
    let numeric = admits(SchemaType::Integer) || admits(SchemaType::Number);
    let boolean = admits(SchemaType::Boolean);
    let typed = match text.as_str() {
        "true" if boolean => serde_json::Value::Bool(true),
        "false" if boolean => serde_json::Value::Bool(false),
        _ if numeric => text.parse::<serde_json::Number>().map_or_else(
            |_| serde_json::Value::String(text),
            serde_json::Value::Number,
        ),
        _ => serde_json::Value::String(text),
    };
    object.default = Some(typed);
    OpenApiSchema::Object(object)
}
