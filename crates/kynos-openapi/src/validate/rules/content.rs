//! Media type rules.
//!
//! A media type's schema is not read here. Whether it is deliberately
//! unconstrained is a question about every schema a document holds, and
//! `rules/schemas.rs` answers it once for all of them.

use crate::{
    model::body::media_type::MediaType,
    validate::{
        rules::parameters::check_header_map,
        violation::{Violation, pointer_token},
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
        violations.push(Violation::error(
            location,
            crate::validate::violation::SpecError::ConflictingEncoding,
        ));
    }

    for (property, encoding) in &content.encoding {
        check_header_map(
            &format!("{location}/encoding/{}/headers", pointer_token(property)),
            &encoding.headers,
            violations,
        );
    }
}
