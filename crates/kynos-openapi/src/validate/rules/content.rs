//! Media type rules, including the annotation that marks a schema as
//! deliberately unconstrained.

use crate::{
    annotation::UNCHECKED_SCHEMA_ANNOTATION,
    model::{body::media_type::MediaType, schema::Schema},
    validate::{
        rules::parameters::check_header_map,
        violation::{SpecError, Violation, pointer_token},
    },
};

pub(in crate::validate) fn check_media_type(
    location: &str,
    content: &MediaType,
    violations: &mut Vec<Violation>,
) {
    // The `example`/`examples` exclusion used to be checked here. A `MediaType`
    // carries one [`Examples`] holding one form or the other, so a document
    // setting both cannot reach this function: it fails to deserialize, and
    // there is no way to build one.

    if let Some(schema) = &content.schema {
        if is_unchecked(schema) {
            violations.push(Violation::warning(location, SpecError::UncheckedSchema));
        }
    }

    // The one exclusion the specification states that this model does *not*
    // spell as a type. A sum type is the crate's usual answer and would be the
    // wrong one here: `prefixEncoding` and `itemEncoding` are 3.2-only, so the
    // field's type would differ between a 3.1 and a 3.2 build, and a type that
    // changes shape with a feature is exactly the non-additivity the model
    // works to avoid. The conflict is also unrepresentable under 3.1, where
    // there is nothing for `encoding` to conflict with.
    #[cfg(feature = "openapi32")]
    if !content.encoding.is_empty()
        && (content.prefix_encoding.is_some() || content.item_encoding.is_some())
    {
        violations.push(Violation::error(location, SpecError::ConflictingEncoding));
    }

    for (property, encoding) in &content.encoding {
        check_header_map(
            &format!("{location}/encoding/{}/headers", pointer_token(property)),
            &encoding.headers,
            violations,
        );
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
