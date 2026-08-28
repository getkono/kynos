use super::{
    Callback, Components, Encoding, Example, ExampleValue, Examples, Header, Link, MediaType,
    OAuthFlows, Operation, Parameter, ParameterIn, PathItem, RefOr, Response, Responses, Schema,
    SecurityScheme, Server, for_each, for_each_item, pointer_token,
};

/// The 3.2-only fields one security scheme can carry.
///
/// `deprecated` is read through a match rather than through a shared accessor
/// because [`SecurityScheme`] is an enum with the field repeated on every
/// variant, and a match is what makes a sixth variant a compile error here
/// rather than a construct this walk silently stops reporting.
#[cfg(feature = "openapi32")]
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
/// The device authorization *flow* is 3.2's addition to the set; the device
/// authorization *URL* is 3.2's addition to a flow, and can ride on one of the
/// four flows 3.1 already had. Reporting only the first would let the second
/// through wherever it does.
#[cfg(feature = "openapi32")]
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
/// A Server Object is not reachable from one place. The specification hangs
/// one off the document, off a Path Item, off an Operation and off a Link, and
/// `name` is 3.2-only in all four. Reading it at the root alone let the other
/// three emit as 3.1 carrying a field 3.1 does not define — and the 3.1
/// meta-schema sets `unevaluatedProperties: false` on `$defs/server`, so the
/// result was invalid rather than merely generous.
#[cfg(feature = "openapi32")]
pub(super) fn collect_server_blockers(location: &str, server: &Server, blockers: &mut Vec<String>) {
    if server.name.is_some() {
        blockers.push(format!("{location}/name"));
    }
}

/// The same for a `servers` array, at the index each one sits at.
#[cfg(feature = "openapi32")]
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
#[cfg(feature = "openapi32")]
pub(super) fn collect_link_blockers(location: &str, link: &Link, blockers: &mut Vec<String>) {
    if let Some(server) = &link.server {
        collect_server_blockers(&format!("{location}/server"), server, blockers);
    }
}

/// Every reusable object, each reached the same way its inline twin is.
///
/// `mediaTypes` is the whole map rather than anything within it: the section
/// itself arrived in 3.2, so its presence is the blocker and descending into it
/// would name one document twice.
#[cfg(feature = "openapi32")]
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
#[cfg(feature = "openapi32")]
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

    // Parameters hoisted above the operations apply to every one of them, so a
    // 3.2 location declared here is no less 3.2 for being declared once.
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

    // `operations()` is `Method::all()`-driven, so it stops at the methods with
    // a field of their own. The map's own presence is already a blocker above,
    // which masks this today — but a construct is reported where it lives, and
    // an operation written here is as real as one written beside it.
    for (method, operation) in &item.additional_operations {
        collect_operation_blockers(
            &format!("{location}/additionalOperations/{}", pointer_token(method)),
            operation,
            blockers,
        );
    }
}

/// The Path Items a callback expression maps to.
#[cfg(feature = "openapi32")]
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

#[cfg(feature = "openapi32")]
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
/// The two are separate fields, and walking only the map is what let a 3.2
/// construct in a `default` response through.
#[cfg(feature = "openapi32")]
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

#[cfg(feature = "openapi32")]
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
/// The location is passed in rather than appended here because a parameter is
/// named by its position under an operation and by its component key under
/// `components`, and only the caller knows which.
#[cfg(feature = "openapi32")]
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

#[cfg(feature = "openapi32")]
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

#[cfg(feature = "openapi32")]
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

    // The three fields above are the Media Type Object's. Everything below is
    // one level further down, which is where this stopped: an Encoding Object
    // carries the same three names of its own, an Example Object carries the
    // two 3.2 added beside `value`, and a Schema Object holds the two that ride
    // on `xml` and `discriminator`.
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
/// Nested encodings are not walked, and that is not an omission: each of these
/// three *is* the nesting, so reporting the outer field already refuses the
/// emission and naming what sits beneath it would say the same thing twice.
#[cfg(feature = "openapi32")]
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
/// `externalValue` is a form 3.1 can express, so an external example is not
/// itself a blocker -- only the `dataValue` that 3.2 lets ride along with it.
#[cfg(feature = "openapi32")]
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
/// Walked over the serialized schema rather than over `SchemaObject`'s fields.
/// Nineteen of those fields hold a subschema, and a hand-written walk over them
/// is exactly the shape that made this function necessary in the first place:
/// correct when written and silently short by one the next time a keyword is
/// added. The serialized form has no such edge to miss.
///
/// Both names are matched only directly beneath a key that holds the object
/// defining them, so a `properties` entry that happens to be spelled `xml` is
/// not mistaken for an XML Object unless it also carries `nodeType` -- and a
/// schema that does is refused rather than downgraded, which is the safe way to
/// be wrong here.
#[cfg(feature = "openapi32")]
pub(super) fn collect_schema_blockers(location: &str, schema: &Schema, blockers: &mut Vec<String>) {
    let Ok(value) = serde_json::to_value(schema) else {
        return;
    };
    collect_schema_value_blockers(location, &value, blockers);
}

#[cfg(feature = "openapi32")]
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
