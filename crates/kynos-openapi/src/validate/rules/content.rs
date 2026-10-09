//! Media type rules.
//!
//! A media type's schema is not read here; `rules/schemas.rs` checks every
//! schema at once.

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
    // The one stated exclusion not spelled as a type: `prefixEncoding` and
    // `itemEncoding` are 3.2-only, so a sum type would change shape with the
    // `openapi32` feature, breaking additivity.
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
