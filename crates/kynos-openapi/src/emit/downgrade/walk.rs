use super::{
    Callback, Components, Encoding, Example, ExampleValue, Examples, Header, Link, MediaType,
    OAuthFlows, Operation, Parameter, ParameterIn, PathItem, RefOr, Response, Responses, Schema,
    SecurityScheme, Server, for_each, for_each_item, pointer_token,
};

/// The 3.2-only fields one security scheme can carry.
///
/// `deprecated` is read through a match so that a sixth [`SecurityScheme`]
/// variant is a compile error here.
pub(super) fn collect_security_scheme_blockers(
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
/// Both the device authorization *flow* and the device authorization *URL*,
/// which can ride on any of the four flows 3.1 already had.
pub(super) fn collect_oauth_flow_blockers(
    location: &str,
    flows: &OAuthFlows,
    blockers: &mut Vec<String>,
) {
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

/// One Server Object, wherever it hangs.
///
/// The document, a Path Item, an Operation and a Link each hang one, and
/// `name` is 3.2-only in all four.
pub(super) fn collect_server_blockers(location: &str, server: &Server, blockers: &mut Vec<String>) {
    if server.name.is_some() {
        blockers.push(format!("{location}/name"));
    }
}

/// The same for a `servers` array, at the index each one sits at.
pub(super) fn collect_servers_blockers(
    location: &str,
    servers: &[Server],
    blockers: &mut Vec<String>,
) {
    for (index, server) in servers.iter().enumerate() {
        collect_server_blockers(&format!("{location}/servers/{index}"), server, blockers);
    }
}

/// One Link Object, which is 3.1 apart from the Server Object it may carry.
pub(super) fn collect_link_blockers(location: &str, link: &Link, blockers: &mut Vec<String>) {
    if let Some(server) = &link.server {
        collect_server_blockers(&format!("{location}/server"), server, blockers);
    }
}

/// Every reusable object, each reached the same way its inline twin is.
///
/// `mediaTypes` is not descended into: the section itself is 3.2-only.
pub(super) fn collect_components_blockers(
    location: &str,
    components: &Components,
    blockers: &mut Vec<String>,
) {
    if !components.media_types.is_empty() {
        blockers.push(format!("{location}/mediaTypes"));
    }

    for_each_item(
        &format!("{location}/securitySchemes"),
        &components.security_schemes,
        |at, scheme| collect_security_scheme_blockers(&at, scheme, blockers),
    );
    for_each(
        &format!("{location}/schemas"),
        &components.schemas,
        |at, schema| collect_schema_blockers(&at, schema, blockers),
    );
    for_each_item(
        &format!("{location}/responses"),
        &components.responses,
        |at, response| collect_response_blockers(&at, response, blockers),
    );
    for_each_item(
        &format!("{location}/parameters"),
        &components.parameters,
        |at, parameter| collect_parameter_blockers(&at, parameter, blockers),
    );
    for_each_item(
        &format!("{location}/headers"),
        &components.headers,
        |at, header| collect_header_blockers(&at, header, blockers),
    );
    for_each_item(
        &format!("{location}/examples"),
        &components.examples,
        |at, example| collect_example_blockers(&at, example, blockers),
    );
    for_each_item(
        &format!("{location}/requestBodies"),
        &components.request_bodies,
        |at, body| {
            for (media_type, content) in &body.content {
                collect_media_type_blockers(
                    &format!("{at}/content/{}", pointer_token(media_type)),
                    content,
                    blockers,
                );
            }
        },
    );
    for_each(
        &format!("{location}/pathItems"),
        &components.path_items,
        |at, item| collect_path_item_blockers(&at, item, blockers),
    );
    for_each_item(
        &format!("{location}/callbacks"),
        &components.callbacks,
        |at, callback| collect_callback_blockers(&at, callback, blockers),
    );
    for_each_item(
        &format!("{location}/links"),
        &components.links,
        |at, link| {
            collect_link_blockers(&at, link, blockers);
        },
    );
}

/// One Path Item, wherever it hangs: `paths`, `webhooks`, a component, or a
/// callback expression.
pub(super) fn collect_path_item_blockers(
    location: &str,
    item: &PathItem,
    blockers: &mut Vec<String>,
) {
    if item.query.is_some() {
        blockers.push(format!("{location}/query"));
    }
    if !item.additional_operations.is_empty() {
        blockers.push(format!("{location}/additionalOperations"));
    }

    // Path-level parameters can carry a 3.2 location too.
    for parameter in item.parameters.iter().filter_map(RefOr::as_item) {
        collect_parameter_blockers(
            &format!("{location}/parameters/{}", parameter.name),
            parameter,
            blockers,
        );
    }

    collect_servers_blockers(location, &item.servers, blockers);

    for (method, operation) in item.operations() {
        collect_operation_blockers(
            &format!("{location}/{}", method.as_wire_str().to_lowercase()),
            operation,
            blockers,
        );
    }

    // `operations()` stops at the methods with a field of their own; a
    // construct is reported where it lives, even under a map already reported.
    for (method, operation) in &item.additional_operations {
        collect_operation_blockers(
            &format!("{location}/additionalOperations/{}", pointer_token(method)),
            operation,
            blockers,
        );
    }
}

/// The Path Items a callback expression maps to.
pub(super) fn collect_callback_blockers(
    location: &str,
    callback: &Callback,
    blockers: &mut Vec<String>,
) {
    for (expression, item) in &callback.items {
        if let RefOr::Item(item) = item {
            collect_path_item_blockers(
                &format!("{location}/{}", pointer_token(expression)),
                item,
                blockers,
            );
        }
    }
}

pub(super) fn collect_operation_blockers(
    location: &str,
    operation: &Operation,
    blockers: &mut Vec<String>,
) {
    for parameter in operation.parameters.iter().filter_map(RefOr::as_item) {
        collect_parameter_blockers(
            &format!("{location}/parameters/{}", parameter.name),
            parameter,
            blockers,
        );
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

    collect_responses_blockers(location, &operation.responses, blockers);
    collect_servers_blockers(location, &operation.servers, blockers);

    for (name, callback) in &operation.callbacks {
        if let RefOr::Item(callback) = callback {
            collect_callback_blockers(
                &format!("{location}/callbacks/{}", pointer_token(name)),
                callback,
                blockers,
            );
        }
    }
}

/// The keyed responses *and* the `default` beside them.
///
/// The two are separate fields.
pub(super) fn collect_responses_blockers(
    location: &str,
    responses: &Responses,
    blockers: &mut Vec<String>,
) {
    for (status, response) in &responses.responses {
        if let Some(response) = response.as_item() {
            collect_response_blockers(
                &format!("{location}/responses/{status}"),
                response,
                blockers,
            );
        }
    }

    if let Some(default) = responses.default_response.as_ref().and_then(RefOr::as_item) {
        collect_response_blockers(&format!("{location}/responses/default"), default, blockers);
    }
}

pub(super) fn collect_response_blockers(
    location: &str,
    response: &Response,
    blockers: &mut Vec<String>,
) {
    if response.summary.is_some() {
        blockers.push(format!("{location}/summary"));
    }

    for (media_type, content) in &response.content {
        collect_media_type_blockers(
            &format!("{location}/content/{}", pointer_token(media_type)),
            content,
            blockers,
        );
    }

    for (name, header) in &response.headers {
        if let RefOr::Item(header) = header {
            collect_header_blockers(
                &format!("{location}/headers/{}", pointer_token(name)),
                header,
                blockers,
            );
        }
    }

    for_each_item(&format!("{location}/links"), &response.links, |at, link| {
        collect_link_blockers(&at, link, blockers);
    });
}

/// A parameter, located at the pointer the caller built for it.
///
/// Only the caller knows whether it is named by position or component key.
pub(super) fn collect_parameter_blockers(
    location: &str,
    parameter: &Parameter,
    blockers: &mut Vec<String>,
) {
    if parameter.location == ParameterIn::Querystring {
        blockers.push(location.to_owned());
    }
    if parameter.style() == Some(crate::model::parameter::style::Style::Cookie) {
        blockers.push(format!("{location}/style"));
    }

    if let Some((media_type, content)) = parameter.content() {
        collect_media_type_blockers(
            &format!("{location}/content/{}", pointer_token(media_type)),
            content,
            blockers,
        );
    }

    if let Some(schema) = parameter.schema() {
        collect_schema_blockers(&format!("{location}/schema"), schema, blockers);
    }

    if let Some(Examples::Named(examples)) = parameter.examples() {
        for (name, example) in examples {
            if let Some(example) = example.as_item() {
                collect_example_blockers(
                    &format!("{location}/examples/{}", pointer_token(name)),
                    example,
                    blockers,
                );
            }
        }
    }
}

pub(super) fn collect_header_blockers(location: &str, header: &Header, blockers: &mut Vec<String>) {
    if let Some((media_type, content)) = header.content() {
        collect_media_type_blockers(
            &format!("{location}/content/{}", pointer_token(media_type)),
            content,
            blockers,
        );
    }

    if let Some(schema) = header.schema() {
        collect_schema_blockers(&format!("{location}/schema"), schema, blockers);
    }

    if let Some(Examples::Named(examples)) = header.examples() {
        for (name, example) in examples {
            if let Some(example) = example.as_item() {
                collect_example_blockers(
                    &format!("{location}/examples/{}", pointer_token(name)),
                    example,
                    blockers,
                );
            }
        }
    }
}

pub(super) fn collect_media_type_blockers(
    location: &str,
    content: &MediaType,
    blockers: &mut Vec<String>,
) {
    for (field, present) in [
        ("itemSchema", content.item_schema.is_some()),
        ("prefixEncoding", content.prefix_encoding.is_some()),
        ("itemEncoding", content.item_encoding.is_some()),
    ] {
        if present {
            blockers.push(format!("{location}/{field}"));
        }
    }

    // The fields above are the Media Type Object's; below, its nested
    // Encoding, Example and Schema Objects carry 3.2 fields of their own.
    for (property, encoding) in &content.encoding {
        collect_encoding_blockers(
            &format!("{location}/encoding/{}", pointer_token(property)),
            encoding,
            blockers,
        );
    }

    if let Some(Examples::Named(examples)) = content.examples() {
        for (name, example) in examples {
            if let Some(example) = example.as_item() {
                collect_example_blockers(
                    &format!("{location}/examples/{}", pointer_token(name)),
                    example,
                    blockers,
                );
            }
        }
    }

    // Not `item_schema`: its presence is already a blocker above, so walking it
    // would name the same document twice.
    if let Some(schema) = &content.schema {
        collect_schema_blockers(&format!("{location}/schema"), schema, blockers);
    }
}

/// The Encoding Object's own three 3.2 fields.
///
/// Nested encodings are not walked: each of these three *is* the nesting.
pub(super) fn collect_encoding_blockers(
    location: &str,
    encoding: &Encoding,
    blockers: &mut Vec<String>,
) {
    for (field, present) in [
        ("encoding", !encoding.encoding.is_empty()),
        ("prefixEncoding", encoding.prefix_encoding.is_some()),
        ("itemEncoding", encoding.item_encoding.is_some()),
    ] {
        if present {
            blockers.push(format!("{location}/{field}"));
        }
    }
}

/// The two example forms 3.2 added beside `value`.
///
/// `externalValue` is 3.1, so only a `dataValue` beside it blocks.
pub(super) fn collect_example_blockers(
    location: &str,
    example: &Example,
    blockers: &mut Vec<String>,
) {
    let (data, serialized) = match example.value() {
        Some(ExampleValue::External { data, .. }) => (data.is_some(), false),
        Some(ExampleValue::Data { serialized, .. }) => (true, serialized.is_some()),
        Some(ExampleValue::Serialized(_)) => (false, true),
        Some(ExampleValue::Embedded(_)) | None => (false, false),
    };

    for (field, present) in [("dataValue", data), ("serializedValue", serialized)] {
        if present {
            blockers.push(format!("{location}/{field}"));
        }
    }
}

/// `xml.nodeType` and `discriminator.defaultMapping`, wherever they are nested.
///
/// Walked over the serialized schema, so a subschema keyword added later
/// cannot be missed.
///
/// Both names are matched only directly beneath `xml` or `discriminator`; a
/// `properties` entry spelled `xml` carrying `nodeType` is refused, which is
/// the safe way to be wrong.
pub(super) fn collect_schema_blockers(location: &str, schema: &Schema, blockers: &mut Vec<String>) {
    let Ok(value) = serde_json::to_value(schema) else {
        return;
    };
    collect_schema_value_blockers(location, &value, blockers);
}

pub(super) fn collect_schema_value_blockers(
    location: &str,
    value: &serde_json::Value,
    blockers: &mut Vec<String>,
) {
    match value {
        serde_json::Value::Object(fields) => {
            for (holder, field) in [("xml", "nodeType"), ("discriminator", "defaultMapping")] {
                let carries = fields
                    .get(holder)
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|held| held.contains_key(field));
                if carries {
                    blockers.push(format!("{location}/{holder}/{field}"));
                }
            }

            for (key, nested) in fields {
                collect_schema_value_blockers(
                    &format!("{location}/{}", pointer_token(key)),
                    nested,
                    blockers,
                );
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_schema_value_blockers(&format!("{location}/{index}"), item, blockers);
            }
        }
        _ => {}
    }
}
