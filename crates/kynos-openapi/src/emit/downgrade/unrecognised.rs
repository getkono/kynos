//! Fields no object in the model recognises, wherever an `extensions` map
//! absorbed one.
//!
//! A flattened [`Extensions`] keeps any key its object leaves unclaimed,
//! including every 3.2 field in a build without `openapi32`. A key lacking the
//! `x-` prefix is no extension and 3.1 cannot read it, so each is reported.
//!
//! Each struct whose fields are all visible here is destructured without `..`,
//! so a field added to the model is a compile error in this file.

use std::fmt::Write as _;

use crate::{
    model::{
        body::{RequestBody, encoding::Encoding, media_type::MediaType},
        callback::Callback,
        components::Components,
        document::Document,
        example::{Example, Examples},
        extensions::{EXTENSION_PREFIX, Extensions},
        external_docs::ExternalDocumentation,
        info::{Contact, Info},
        link::Link,
        parameter::{Parameter, header::Header},
        paths::{item::PathItem, operation::Operation},
        reference::RefOr,
        response::{Response, Responses},
        schema::{Schema, discriminator::Discriminator, object::SchemaObject, xml::Xml},
        security::{
            SecurityScheme,
            oauth::{OAuthFlow, OAuthFlows},
        },
        server::{Server, ServerVariable},
        tag::Tag,
    },
    validate::violation::pointer_token,
};

/// Where the walk stands, as a chain of borrowed tokens on the stack.
///
/// Rendered only when a field is reported, so a clean document costs no
/// allocation (held by `tests/alloc.rs`).
#[derive(Clone, Copy)]
struct At<'a> {
    parent: Option<&'a At<'a>>,
    token: Token<'a>,
}

#[derive(Clone, Copy)]
enum Token<'a> {
    Root,
    Key(&'a str),
    Index(usize),
}

impl<'a> At<'a> {
    const ROOT: At<'static> = At {
        parent: None,
        token: Token::Root,
    };

    fn key(&'a self, key: &'a str) -> Self {
        Self {
            parent: Some(self),
            token: Token::Key(key),
        }
    }

    fn index(&'a self, index: usize) -> Self {
        Self {
            parent: Some(self),
            token: Token::Index(index),
        }
    }

    fn render(&self, pointer: &mut String) {
        if let Some(parent) = self.parent {
            parent.render(pointer);
        }
        match self.token {
            Token::Root => pointer.push('#'),
            Token::Key(key) => {
                pointer.push('/');
                pointer.push_str(&pointer_token(key));
            }
            Token::Index(index) => {
                // Writing to a `String` cannot fail.
                let _ = write!(pointer, "/{index}");
            }
        }
    }
}

/// Reports each key of `extensions` that lacks the `x-` prefix.
fn check(at: At<'_>, extensions: &Extensions, found: &mut Vec<String>) {
    for key in extensions.0.keys() {
        if !key.starts_with(EXTENSION_PREFIX) {
            let mut pointer = String::new();
            at.key(key).render(&mut pointer);
            found.push(pointer);
        }
    }
}

/// Every unrecognised field in `document`, at the pointer it was written at.
pub(super) fn collect_unrecognised_fields(document: &Document, found: &mut Vec<String>) {
    let Document {
        openapi: _,
        #[cfg(feature = "openapi32")]
            self_uri: _,
        info,
        json_schema_dialect: _,
        servers,
        paths,
        webhooks,
        components,
        security: _,
        tags,
        external_docs,
        extensions,
    } = document;
    let at = At::ROOT;

    check(at, extensions, found);
    info_fields(at.key("info"), info, found);
    servers_fields(at.key("servers"), servers, found);

    let at_paths = at.key("paths");
    check(at_paths, &paths.extensions, found);
    for (raw, item) in &paths.items {
        path_item_fields(at_paths.key(raw), item, found);
    }

    let at_webhooks = at.key("webhooks");
    for (name, item) in webhooks {
        path_item_fields(at_webhooks.key(name), item, found);
    }

    components_fields(at.key("components"), components, found);

    let at_tags = at.key("tags");
    for (index, tag) in tags.iter().enumerate() {
        tag_fields(at_tags.index(index), tag, found);
    }
    if let Some(docs) = external_docs {
        external_docs_fields(at.key("externalDocs"), docs, found);
    }
}

fn info_fields(at: At<'_>, info: &Info, found: &mut Vec<String>) {
    let Info {
        title: _,
        summary: _,
        description: _,
        terms_of_service: _,
        contact,
        license,
        version: _,
        extensions,
    } = info;

    check(at, extensions, found);
    if let Some(contact) = contact {
        let Contact {
            name: _,
            url: _,
            email: _,
            extensions,
        } = contact;
        check(at.key("contact"), extensions, found);
    }
    // `License` keeps its name and link private; its extensions are all it
    // holds that can carry a field.
    if let Some(license) = license {
        check(at.key("license"), &license.extensions, found);
    }
}

fn servers_fields(at: At<'_>, servers: &[Server], found: &mut Vec<String>) {
    for (index, server) in servers.iter().enumerate() {
        server_fields(at.index(index), server, found);
    }
}

fn server_fields(at: At<'_>, server: &Server, found: &mut Vec<String>) {
    let Server {
        url: _,
        #[cfg(feature = "openapi32")]
            name: _,
        description: _,
        variables,
        extensions,
    } = server;

    check(at, extensions, found);
    let at_variables = at.key("variables");
    for (name, variable) in variables {
        let ServerVariable {
            enumeration: _,
            default_value: _,
            description: _,
            extensions,
        } = variable;
        check(at_variables.key(name), extensions, found);
    }
}

fn external_docs_fields(at: At<'_>, docs: &ExternalDocumentation, found: &mut Vec<String>) {
    let ExternalDocumentation {
        description: _,
        url: _,
        extensions,
    } = docs;
    check(at, extensions, found);
}

fn tag_fields(at: At<'_>, tag: &Tag, found: &mut Vec<String>) {
    let Tag {
        name: _,
        #[cfg(feature = "openapi32")]
            summary: _,
        description: _,
        #[cfg(feature = "openapi32")]
            parent: _,
        #[cfg(feature = "openapi32")]
            kind: _,
        external_docs,
        extensions,
    } = tag;

    check(at, extensions, found);
    if let Some(docs) = external_docs {
        external_docs_fields(at.key("externalDocs"), docs, found);
    }
}

fn components_fields(at: At<'_>, components: &Components, found: &mut Vec<String>) {
    let Components {
        schemas,
        responses,
        parameters,
        examples,
        request_bodies,
        headers,
        security_schemes,
        links,
        callbacks,
        path_items,
        #[cfg(feature = "openapi32")]
        media_types,
        extensions,
    } = components;

    check(at, extensions, found);

    let section = at.key("schemas");
    for (name, schema) in schemas {
        schema_fields(section.key(name), schema, found);
    }
    for_each_item(at.key("responses"), responses, found, response_fields);
    for_each_item(at.key("parameters"), parameters, found, parameter_fields);
    for_each_item(at.key("examples"), examples, found, example_fields);
    for_each_item(
        at.key("requestBodies"),
        request_bodies,
        found,
        request_body_fields,
    );
    for_each_item(at.key("headers"), headers, found, header_fields);
    for_each_item(
        at.key("securitySchemes"),
        security_schemes,
        found,
        security_scheme_fields,
    );
    for_each_item(at.key("links"), links, found, link_fields);
    for_each_item(at.key("callbacks"), callbacks, found, callback_fields);

    let section = at.key("pathItems");
    for (name, item) in path_items {
        path_item_fields(section.key(name), item, found);
    }

    #[cfg(feature = "openapi32")]
    for_each_item(at.key("mediaTypes"), media_types, found, media_type_fields);
}

/// Visits each inline item of a `RefOr` map under `at`.
///
/// A `RefOr::Ref` is skipped: a Reference Object carries no `extensions`, and
/// what it names is walked where it is defined.
fn for_each_item<'a, T: 'a>(
    at: At<'_>,
    entries: impl IntoIterator<Item = (&'a String, &'a RefOr<T>)>,
    found: &mut Vec<String>,
    visit: fn(At<'_>, &T, &mut Vec<String>),
) {
    for (name, entry) in entries {
        if let Some(item) = entry.as_item() {
            visit(at.key(name), item, found);
        }
    }
}

fn path_item_fields(at: At<'_>, item: &PathItem, found: &mut Vec<String>) {
    let PathItem {
        reference: _,
        summary: _,
        description: _,
        get,
        put,
        post,
        delete,
        options,
        head,
        patch,
        trace,
        #[cfg(feature = "openapi32")]
        query,
        #[cfg(feature = "openapi32")]
        additional_operations,
        servers,
        parameters,
        extensions,
    } = item;

    check(at, extensions, found);

    for (method, operation) in [
        ("get", get),
        ("put", put),
        ("post", post),
        ("delete", delete),
        ("options", options),
        ("head", head),
        ("patch", patch),
        ("trace", trace),
        #[cfg(feature = "openapi32")]
        ("query", query),
    ] {
        if let Some(operation) = operation {
            operation_fields(at.key(method), operation, found);
        }
    }

    #[cfg(feature = "openapi32")]
    {
        let section = at.key("additionalOperations");
        for (method, operation) in additional_operations {
            operation_fields(section.key(method), operation, found);
        }
    }

    servers_fields(at.key("servers"), servers, found);
    parameters_fields(at.key("parameters"), parameters, found);
}

fn parameters_fields(at: At<'_>, parameters: &[RefOr<Parameter>], found: &mut Vec<String>) {
    for (index, parameter) in parameters.iter().enumerate() {
        if let Some(parameter) = parameter.as_item() {
            parameter_fields(at.index(index), parameter, found);
        }
    }
}

fn operation_fields(at: At<'_>, operation: &Operation, found: &mut Vec<String>) {
    let Operation {
        tags: _,
        summary: _,
        description: _,
        external_docs,
        operation_id: _,
        parameters,
        request_body,
        responses,
        callbacks,
        deprecated: _,
        security: _,
        servers,
        extensions,
    } = operation;

    check(at, extensions, found);
    if let Some(docs) = external_docs {
        external_docs_fields(at.key("externalDocs"), docs, found);
    }
    parameters_fields(at.key("parameters"), parameters, found);
    if let Some(body) = request_body.as_ref().and_then(RefOr::as_item) {
        request_body_fields(at.key("requestBody"), body, found);
    }
    responses_fields(at.key("responses"), responses, found);
    for_each_item(at.key("callbacks"), callbacks, found, callback_fields);
    servers_fields(at.key("servers"), servers, found);
}

fn request_body_fields(at: At<'_>, body: &RequestBody, found: &mut Vec<String>) {
    let RequestBody {
        description: _,
        content,
        required: _,
        extensions,
    } = body;

    check(at, extensions, found);
    content_fields(at.key("content"), content, found);
}

fn content_fields<'a>(
    at: At<'_>,
    content: impl IntoIterator<Item = (&'a String, &'a MediaType)>,
    found: &mut Vec<String>,
) {
    for (media_type, value) in content {
        media_type_fields(at.key(media_type), value, found);
    }
}

fn responses_fields(at: At<'_>, responses: &Responses, found: &mut Vec<String>) {
    let Responses {
        default_response,
        responses,
        extensions,
    } = responses;

    check(at, extensions, found);
    if let Some(default) = default_response.as_ref().and_then(RefOr::as_item) {
        response_fields(at.key("default"), default, found);
    }
    for_each_item(at, responses, found, response_fields);
}

fn response_fields(at: At<'_>, response: &Response, found: &mut Vec<String>) {
    let Response {
        #[cfg(feature = "openapi32")]
            summary: _,
        description: _,
        headers,
        content,
        links,
        extensions,
    } = response;

    check(at, extensions, found);
    for_each_item(at.key("headers"), headers, found, header_fields);
    content_fields(at.key("content"), content, found);
    for_each_item(at.key("links"), links, found, link_fields);
}

fn callback_fields(at: At<'_>, callback: &Callback, found: &mut Vec<String>) {
    let Callback { items, extensions } = callback;

    check(at, extensions, found);
    for_each_item(at, items, found, path_item_fields);
}

/// `Link` keeps its target private, and the target is a string.
fn link_fields(at: At<'_>, link: &Link, found: &mut Vec<String>) {
    check(at, &link.extensions, found);
    if let Some(server) = &link.server {
        server_fields(at.key("server"), server, found);
    }
}

/// `Example` keeps its value private, and no form of the value holds a map.
fn example_fields(at: At<'_>, example: &Example, found: &mut Vec<String>) {
    check(at, &example.extensions, found);
}

fn examples_fields(at: At<'_>, examples: Option<&Examples>, found: &mut Vec<String>) {
    if let Some(Examples::Named(examples)) = examples {
        for_each_item(at.key("examples"), examples, found, example_fields);
    }
}

/// `Parameter` keeps its shape private; the accessors reach all of it.
fn parameter_fields(at: At<'_>, parameter: &Parameter, found: &mut Vec<String>) {
    check(at, &parameter.extensions, found);
    if let Some(schema) = parameter.schema() {
        schema_fields(at.key("schema"), schema, found);
    }
    if let Some((media_type, value)) = parameter.content() {
        media_type_fields(at.key("content").key(media_type), value, found);
    }
    examples_fields(at, parameter.examples(), found);
}

/// The same for a `Header`, which keeps its shape private for the same reason.
fn header_fields(at: At<'_>, header: &Header, found: &mut Vec<String>) {
    check(at, &header.extensions, found);
    if let Some(schema) = header.schema() {
        schema_fields(at.key("schema"), schema, found);
    }
    if let Some((media_type, value)) = header.content() {
        media_type_fields(at.key("content").key(media_type), value, found);
    }
    examples_fields(at, header.examples(), found);
}

/// `MediaType` keeps its examples private, behind [`MediaType::examples`].
fn media_type_fields(at: At<'_>, media_type: &MediaType, found: &mut Vec<String>) {
    check(at, &media_type.extensions, found);
    if let Some(schema) = &media_type.schema {
        schema_fields(at.key("schema"), schema, found);
    }
    examples_fields(at, media_type.examples(), found);
    encodings_fields(at.key("encoding"), &media_type.encoding, found);

    #[cfg(feature = "openapi32")]
    {
        if let Some(schema) = &media_type.item_schema {
            schema_fields(at.key("itemSchema"), schema, found);
        }
        if let Some(prefix) = &media_type.prefix_encoding {
            let section = at.key("prefixEncoding");
            for (index, encoding) in prefix.iter().enumerate() {
                encoding_fields(section.index(index), encoding, found);
            }
        }
        if let Some(item) = &media_type.item_encoding {
            encoding_fields(at.key("itemEncoding"), item, found);
        }
    }
}

fn encodings_fields<'a>(
    at: At<'_>,
    encodings: impl IntoIterator<Item = (&'a String, &'a Encoding)>,
    found: &mut Vec<String>,
) {
    for (property, encoding) in encodings {
        encoding_fields(at.key(property), encoding, found);
    }
}

fn encoding_fields(at: At<'_>, encoding: &Encoding, found: &mut Vec<String>) {
    let Encoding {
        content_type: _,
        headers,
        style: _,
        explode: _,
        allow_reserved: _,
        #[cfg(feature = "openapi32")]
        encoding,
        #[cfg(feature = "openapi32")]
        prefix_encoding,
        #[cfg(feature = "openapi32")]
        item_encoding,
        extensions,
    } = encoding;

    check(at, extensions, found);
    for_each_item(at.key("headers"), headers, found, header_fields);

    #[cfg(feature = "openapi32")]
    {
        encodings_fields(at.key("encoding"), encoding, found);
        if let Some(prefix) = prefix_encoding {
            let section = at.key("prefixEncoding");
            for (index, encoding) in prefix.iter().enumerate() {
                encoding_fields(section.index(index), encoding, found);
            }
        }
        if let Some(item) = item_encoding {
            encoding_fields(at.key("itemEncoding"), item, found);
        }
    }
}

/// Every security scheme type, matched rather than read through an accessor so
/// that a sixth one is a compile error here.
fn security_scheme_fields(at: At<'_>, scheme: &SecurityScheme, found: &mut Vec<String>) {
    match scheme {
        SecurityScheme::ApiKey { extensions, .. }
        | SecurityScheme::Http { extensions, .. }
        | SecurityScheme::MutualTls { extensions, .. }
        | SecurityScheme::OpenIdConnect { extensions, .. } => check(at, extensions, found),
        SecurityScheme::OAuth2 {
            flows, extensions, ..
        } => {
            check(at, extensions, found);
            oauth_flows_fields(at.key("flows"), flows, found);
        }
    }
}

fn oauth_flows_fields(at: At<'_>, flows: &OAuthFlows, found: &mut Vec<String>) {
    let OAuthFlows {
        implicit,
        password,
        client_credentials,
        authorization_code,
        #[cfg(feature = "openapi32")]
        device_authorization,
        extensions,
    } = flows;

    check(at, extensions, found);
    for (name, flow) in [
        ("implicit", implicit),
        ("password", password),
        ("clientCredentials", client_credentials),
        ("authorizationCode", authorization_code),
        #[cfg(feature = "openapi32")]
        ("deviceAuthorization", device_authorization),
    ] {
        if let Some(flow) = flow {
            let OAuthFlow {
                authorization_url: _,
                token_url: _,
                #[cfg(feature = "openapi32")]
                    device_authorization_url: _,
                refresh_url: _,
                scopes: _,
                extensions,
            } = flow;
            check(at.key(name), extensions, found);
        }
    }
}

/// The two objects a Schema Object holds that carry `extensions`, at any
/// depth.
///
/// Its own [`SchemaObject::unknown_keywords`] are not reported: a Schema Object
/// alone may hold fields without the `x-` prefix.
#[expect(
    clippy::too_many_lines,
    reason = "the length is the exhaustive list of a Schema Object's keywords"
)]
fn schema_fields(at: At<'_>, schema: &Schema, found: &mut Vec<String>) {
    let Schema::Object(object) = schema else {
        return;
    };

    #[expect(
        deprecated,
        reason = "`example` is named so that the destructuring stays exhaustive"
    )]
    let SchemaObject {
        schema_dialect: _,
        id: _,
        reference: _,
        anchor: _,
        dynamic_ref: _,
        dynamic_anchor: _,
        comment: _,
        defs,
        all_of,
        any_of,
        one_of,
        not,
        if_schema,
        then_schema,
        else_schema,
        dependent_schemas,
        prefix_items,
        items,
        contains,
        properties,
        pattern_properties,
        additional_properties,
        property_names,
        unevaluated_items,
        unevaluated_properties,
        ty: _,
        const_value: _,
        enumeration: _,
        multiple_of: _,
        maximum: _,
        exclusive_maximum: _,
        minimum: _,
        exclusive_minimum: _,
        max_length: _,
        min_length: _,
        pattern: _,
        max_items: _,
        min_items: _,
        unique_items: _,
        max_contains: _,
        min_contains: _,
        max_properties: _,
        min_properties: _,
        required: _,
        dependent_required: _,
        format: _,
        content_encoding: _,
        content_media_type: _,
        content_schema,
        title: _,
        description: _,
        default: _,
        deprecated: _,
        read_only: _,
        write_only: _,
        examples: _,
        discriminator,
        xml,
        external_docs,
        example: _,
        unknown_keywords: _,
    } = &**object;

    if let Some(discriminator) = discriminator {
        let Discriminator {
            property_name: _,
            mapping: _,
            #[cfg(feature = "openapi32")]
                default_mapping: _,
            extensions,
        } = discriminator;
        check(at.key("discriminator"), extensions, found);
    }
    if let Some(xml) = xml {
        let Xml {
            #[cfg(feature = "openapi32")]
                node_type: _,
            name: _,
            namespace: _,
            prefix: _,
            attribute: _,
            wrapped: _,
            extensions,
        } = xml;
        check(at.key("xml"), extensions, found);
    }
    if let Some(docs) = external_docs {
        external_docs_fields(at.key("externalDocs"), docs, found);
    }

    for (keyword, subschemas) in [
        ("$defs", defs),
        ("dependentSchemas", dependent_schemas),
        ("properties", properties),
        ("patternProperties", pattern_properties),
    ] {
        let section = at.key(keyword);
        for (name, subschema) in subschemas {
            schema_fields(section.key(name), subschema, found);
        }
    }

    for (keyword, subschemas) in [
        ("allOf", all_of),
        ("anyOf", any_of),
        ("oneOf", one_of),
        ("prefixItems", prefix_items),
    ] {
        let section = at.key(keyword);
        for (index, subschema) in subschemas.iter().flatten().enumerate() {
            schema_fields(section.index(index), subschema, found);
        }
    }

    for (keyword, subschema) in [
        ("not", not),
        ("if", if_schema),
        ("then", then_schema),
        ("else", else_schema),
        ("items", items),
        ("contains", contains),
        ("additionalProperties", additional_properties),
        ("propertyNames", property_names),
        ("unevaluatedItems", unevaluated_items),
        ("unevaluatedProperties", unevaluated_properties),
        ("contentSchema", content_schema),
    ] {
        if let Some(subschema) = subschema {
            schema_fields(at.key(keyword), subschema, found);
        }
    }
}
