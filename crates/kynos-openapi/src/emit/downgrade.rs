//! What stands in the way of emitting a document as an earlier version.

use crate::model::document::Document;
#[cfg(feature = "openapi32")]
use crate::validate::violation::pointer_token;

// Everything below the document itself is reached only while collecting 3.2
// blockers, which a build without `openapi32` cannot have any of.
#[cfg(feature = "openapi32")]
use crate::model::{
    body::media_type::MediaType,
    parameter::ParameterIn,
    paths::operation::Operation,
    reference::RefOr,
    security::{SecurityScheme, oauth::OAuthFlows},
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

        for (name, scheme) in &document.components.security_schemes {
            let location = format!("#/components/securitySchemes/{}", pointer_token(name));
            if let RefOr::Item(scheme) = scheme {
                collect_security_scheme_blockers(&location, scheme, &mut blockers);
            }
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

/// The 3.2-only fields one security scheme can carry.
///
/// `deprecated` is read through a match rather than through a shared accessor
/// because [`SecurityScheme`] is an enum with the field repeated on every
/// variant, and a match is what makes a sixth variant a compile error here
/// rather than a construct this walk silently stops reporting.
#[cfg(feature = "openapi32")]
fn collect_security_scheme_blockers(
    location: &str,
    scheme: &SecurityScheme,
    blockers: &mut Vec<String>,
) {
    let deprecated = match scheme {
        SecurityScheme::ApiKey { deprecated, .. }
        | SecurityScheme::Http { deprecated, .. }
        | SecurityScheme::MutualTls { deprecated, .. }
        | SecurityScheme::OpenIdConnect { deprecated, .. } => deprecated,
        SecurityScheme::OAuth2 {
            deprecated,
            flows,
            oauth2_metadata_url,
            ..
        } => {
            if oauth2_metadata_url.is_some() {
                blockers.push(format!("{location}/oauth2MetadataUrl"));
            }
            collect_oauth_flow_blockers(&format!("{location}/flows"), flows, blockers);
            deprecated
        }
    };

    if deprecated.is_some() {
        blockers.push(format!("{location}/deprecated"));
    }
}

/// The 3.2-only constructs an OAuth 2.0 flow set can carry.
///
/// The device authorization *flow* is 3.2's addition to the set; the device
/// authorization *URL* is 3.2's addition to a flow, and can ride on one of the
/// four flows 3.1 already had. Reporting only the first would let the second
/// through wherever it does.
#[cfg(feature = "openapi32")]
fn collect_oauth_flow_blockers(location: &str, flows: &OAuthFlows, blockers: &mut Vec<String>) {
    if flows.device_authorization.is_some() {
        blockers.push(format!("{location}/deviceAuthorization"));
    }

    for (name, flow) in [
        ("implicit", flows.implicit.as_ref()),
        ("password", flows.password.as_ref()),
        ("clientCredentials", flows.client_credentials.as_ref()),
        ("authorizationCode", flows.authorization_code.as_ref()),
        ("deviceAuthorization", flows.device_authorization.as_ref()),
    ] {
        if flow.is_some_and(|flow| flow.device_authorization_url.is_some()) {
            blockers.push(format!("{location}/{name}/deviceAuthorizationUrl"));
        }
    }
}

#[cfg(feature = "openapi32")]
fn collect_operation_blockers(location: &str, operation: &Operation, blockers: &mut Vec<String>) {
    for parameter in operation.parameters.iter().filter_map(RefOr::as_item) {
        if parameter.location == ParameterIn::Querystring {
            blockers.push(format!("{location}/parameters/{}", parameter.name));
        }
        if parameter.style() == Some(crate::model::parameter::style::Style::Cookie) {
            blockers.push(format!("{location}/parameters/{}/style", parameter.name));
        }
    }

    if let Some(RefOr::Item(body)) = &operation.request_body {
        for (media_type, content) in &body.content {
            collect_media_type_blockers(
                &format!(
                    "{location}/requestBody/content/{}",
                    pointer_token(media_type)
                ),
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
                &format!(
                    "{location}/responses/{status}/content/{}",
                    pointer_token(media_type)
                ),
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
