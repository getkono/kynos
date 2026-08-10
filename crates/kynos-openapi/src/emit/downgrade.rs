//! What stands in the way of emitting a document as an earlier version.

use crate::model::document::Document;
#[cfg(feature = "openapi32")]
use crate::validate::violation::pointer_token;

// Everything below the document itself is reached only while collecting 3.2
// blockers, which a build without `openapi32` cannot have any of.
#[cfg(feature = "openapi32")]
use crate::model::{
    body::media_type::MediaType, parameter::ParameterIn, paths::operation::Operation,
    reference::RefOr,
};

/// Lists the OpenAPI 3.2-only constructs a document uses.
///
/// Each entry is a location, suitable for telling the caller what stands in the
/// way of emitting the document as 3.1. Always empty in a build without the
/// `openapi32` feature, since the constructs cannot be represented at all.
#[must_use]
pub fn three_two_only_constructs(document: &Document) -> Vec<String> {
    #[cfg(not(feature = "openapi32"))]
    {
        let _ = document;
        Vec::new()
    }

    #[cfg(feature = "openapi32")]
    {
        let mut blockers = Vec::new();

        if document.self_uri.is_some() {
            blockers.push("#/$self".to_owned());
        }
        for (index, server) in document.servers.iter().enumerate() {
            if server.name.is_some() {
                blockers.push(format!("#/servers/{index}/name"));
            }
        }
        for (index, tag) in document.tags.iter().enumerate() {
            for (field, present) in [
                ("summary", tag.summary.is_some()),
                ("parent", tag.parent.is_some()),
                ("kind", tag.kind.is_some()),
            ] {
                if present {
                    blockers.push(format!("#/tags/{index}/{field}"));
                }
            }
        }
        if !document.components.media_types.is_empty() {
            blockers.push("#/components/mediaTypes".to_owned());
        }

        for (raw, item) in &document.paths.0 {
            let location = format!("#/paths/{}", pointer_token(raw));
            if item.query.is_some() {
                blockers.push(format!("{location}/query"));
            }
            if !item.additional_operations.is_empty() {
                blockers.push(format!("{location}/additionalOperations"));
            }
            for (method, operation) in item.operations() {
                let op = format!("{location}/{}", method.as_wire_str().to_lowercase());
                collect_operation_blockers(&op, operation, &mut blockers);
            }
        }

        blockers
    }
}

#[cfg(feature = "openapi32")]
fn collect_operation_blockers(location: &str, operation: &Operation, blockers: &mut Vec<String>) {
    for parameter in operation.parameters.iter().filter_map(RefOr::as_item) {
        if parameter.location == ParameterIn::Querystring {
            blockers.push(format!("{location}/parameters/{}", parameter.name));
        }
        if parameter.style == Some(crate::model::parameter::style::Style::Cookie) {
            blockers.push(format!("{location}/parameters/{}/style", parameter.name));
        }
    }

    if let Some(RefOr::Item(body)) = &operation.request_body {
        for (media_type, content) in &body.content {
            collect_media_type_blockers(
                &format!("{location}/requestBody/content/{media_type}"),
                content,
                blockers,
            );
        }
    }

    for (status, response) in &operation.responses.responses {
        let Some(response) = response.as_item() else {
            continue;
        };
        if response.summary.is_some() {
            blockers.push(format!("{location}/responses/{status}/summary"));
        }
        for (media_type, content) in &response.content {
            collect_media_type_blockers(
                &format!("{location}/responses/{status}/content/{media_type}"),
                content,
                blockers,
            );
        }
    }
}

#[cfg(feature = "openapi32")]
fn collect_media_type_blockers(location: &str, content: &MediaType, blockers: &mut Vec<String>) {
    for (field, present) in [
        ("itemSchema", content.item_schema.is_some()),
        ("prefixEncoding", content.prefix_encoding.is_some()),
        ("itemEncoding", content.item_encoding.is_some()),
    ] {
        if present {
            blockers.push(format!("{location}/{field}"));
        }
    }
}
