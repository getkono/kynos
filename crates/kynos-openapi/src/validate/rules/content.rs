//! Media type rules, and the request bodies and responses that hold them.
//!
//! A media type's schema is not read here; `rules/schemas.rs` checks every
//! schema at once.

use crate::{
    model::{
        body::{RequestBody, media_type::MediaType},
        document::Document,
        response::Response,
    },
    validate::{
        rules::parameters::check_header_map,
        violation::{Violation, pointer_token},
    },
};

/// Checks each reusable response, request body and media type once, where it
/// is defined.
///
/// Not once per reference; one that is itself a `$ref` is checked where it
/// leads.
pub(in crate::validate) fn check_component_content(
    document: &Document,
    violations: &mut Vec<Violation>,
) {
    let components = &document.components;
    for (name, response) in &components.responses {
        if let Some(response) = response.as_item() {
            check_response(
                &format!("#/components/responses/{}", pointer_token(name)),
                response,
                violations,
            );
        }
    }
    for (name, body) in &components.request_bodies {
        if let Some(body) = body.as_item() {
            check_request_body(
                &format!("#/components/requestBodies/{}", pointer_token(name)),
                body,
                violations,
            );
        }
    }
    #[cfg(feature = "openapi32")]
    for (name, media_type) in &components.media_types {
        if let Some(media_type) = media_type.as_item() {
            check_media_type(
                &format!("#/components/mediaTypes/{}", pointer_token(name)),
                media_type,
                violations,
            );
        }
    }
}

pub(in crate::validate) fn check_request_body(
    location: &str,
    body: &RequestBody,
    violations: &mut Vec<Violation>,
) {
    for (media_type, content) in &body.content {
        check_media_type(
            &format!("{location}/content/{}", pointer_token(media_type)),
            content,
            violations,
        );
    }
}

pub(in crate::validate) fn check_response(
    location: &str,
    response: &Response,
    violations: &mut Vec<Violation>,
) {
    for (media_type, content) in &response.content {
        check_media_type(
            &format!("{location}/content/{}", pointer_token(media_type)),
            content,
            violations,
        );
    }
    // Links and a header's shape hold by construction; a header's name is a
    // map key no type reaches, so it is still checked.
    check_header_map(
        &format!("{location}/headers"),
        &response.headers,
        violations,
    );
}

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
