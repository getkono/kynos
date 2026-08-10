//! Media type rules, including the annotation that marks a schema as
//! deliberately unconstrained.

use crate::{
    model::{body::media_type::MediaType, schema::Schema},
    validate::{
        UNCHECKED_SCHEMA_ANNOTATION,
        violation::{SpecError, Violation},
    },
};

pub(in crate::validate) fn check_media_type(
    location: &str,
    content: &MediaType,
    violations: &mut Vec<Violation>,
) {
    if content.example.is_some() && !content.examples.is_empty() {
        violations.push(Violation::error(location, SpecError::ExampleExclusivity));
    }

    if let Some(schema) = &content.schema {
        if is_unchecked(schema) {
            violations.push(Violation::warning(location, SpecError::UncheckedSchema));
        }
    }
}

fn is_unchecked(schema: &Schema) -> bool {
    match schema {
        Schema::Bool(true) => true,
        Schema::Object(object) => object
            .unknown_keywords
            .contains_key(UNCHECKED_SCHEMA_ANNOTATION),
        Schema::Bool(false) => false,
    }
}
