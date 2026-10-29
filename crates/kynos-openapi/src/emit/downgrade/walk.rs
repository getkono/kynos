//! The 3.2-only fields the model types, which only a build with `openapi32`
//! can hold.
//!
//! Every location is an [`At`] chain, rendered only for a construct reported,
//! so a document holding none costs this walk no allocation (held by
//! `tests/alloc.rs`).

use serde_json::Value;

use crate::{
    Map,
    model::{
        body::{encoding::Encoding, media_type::MediaType},
        components::Components,
        document::Document,
        example::{Example, ExampleValue},
        link::Link,
        parameter::{Parameter, ParameterIn, header::Header, style::Style},
        paths::{item::PathItem, operation::Operation},
        reference::RefOr,
        response::{Response, Responses},
        schema::{Schema, xml::XmlNode},
        security::{SecurityScheme, oauth::OAuthFlows},
        server::Server,
    },
    pointer::{At, pointer_token},
    visit::{self, Visit},
};

/// The 3.2-only fields the model types, appended to `blockers`.
pub(super) fn collect_three_two_fields(document: &Document, blockers: &mut Vec<String>) {
    let root = At::ROOT;
    if document.self_uri.is_some() {
        blockers.push(root.key("$self").pointer());
    }
    collect_servers_blockers(root, &document.servers, blockers);
    let tags = root.key("tags");
    for (index, tag) in document.tags.iter().enumerate() {
        let at = tags.index(index);
        for (field, present) in [
            ("summary", tag.summary.is_some()),
            ("parent", tag.parent.is_some()),
            ("kind", tag.kind.is_some()),
        ] {
            if present {
                blockers.push(at.key(field).pointer());
            }
        }
    }
    collect_components_blockers(root.key("components"), &document.components, blockers);

    // A webhook is a Path Item, and every 3.2 construct one can carry is
    // one a Path Item under `paths` can carry.
    visit::routes(document, &mut Blockers { blockers });
}

/// The 3.2-only fields one security scheme can carry.
///
/// `deprecated` is read through a match so that a sixth [`SecurityScheme`]
/// variant is a compile error here.
fn collect_security_scheme_blockers(
    at: At<'_>,
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
                blockers.push(at.key("oauth2MetadataUrl").pointer());
            }
            collect_oauth_flow_blockers(at.key("flows"), flows, blockers);
            deprecated
        }
    };

    if deprecated.is_some() {
        blockers.push(at.key("deprecated").pointer());
    }
}

/// The 3.2-only constructs an OAuth 2.0 flow set can carry.
///
/// Both the device authorization *flow* and the device authorization *URL*,
/// which can ride on any of the four flows 3.1 already had.
fn collect_oauth_flow_blockers(at: At<'_>, flows: &OAuthFlows, blockers: &mut Vec<String>) {
    if flows.device_authorization.is_some() {
        blockers.push(at.key("deviceAuthorization").pointer());
    }

    for (name, flow) in [
        ("implicit", flows.implicit.as_ref()),
        ("password", flows.password.as_ref()),
        ("clientCredentials", flows.client_credentials.as_ref()),
        ("authorizationCode", flows.authorization_code.as_ref()),
        ("deviceAuthorization", flows.device_authorization.as_ref()),
    ] {
        if flow.is_some_and(|flow| flow.device_authorization_url.is_some()) {
            blockers.push(at.key(name).key("deviceAuthorizationUrl").pointer());
        }
    }
}

/// One Server Object, wherever it hangs.
///
/// The document, a Path Item, an Operation and a Link each hang one, and
/// `name` is 3.2-only in all four.
fn collect_server_blockers(at: At<'_>, server: &Server, blockers: &mut Vec<String>) {
    if server.name.is_some() {
        blockers.push(at.key("name").pointer());
    }
}

/// The same for the `servers` array of the object at `at`, at the index each
/// one sits at.
fn collect_servers_blockers(at: At<'_>, servers: &[Server], blockers: &mut Vec<String>) {
    let at = at.key("servers");
    for (index, server) in servers.iter().enumerate() {
        collect_server_blockers(at.index(index), server, blockers);
    }
}

/// One Link Object, which is 3.1 apart from the Server Object it may carry.
fn collect_link_blockers(at: At<'_>, link: &Link, blockers: &mut Vec<String>) {
    if let Some(server) = &link.server {
        collect_server_blockers(at.key("server"), server, blockers);
    }
}

/// Every reusable object, each reached the same way its inline twin is.
///
/// `mediaTypes` is not descended into: the section itself is 3.2-only.
fn collect_components_blockers(at: At<'_>, components: &Components, blockers: &mut Vec<String>) {
    if !components.media_types.is_empty() {
        blockers.push(at.key("mediaTypes").pointer());
    }

    visit::inline_items(
        at.key("securitySchemes"),
        &components.security_schemes,
        |at, scheme| collect_security_scheme_blockers(at, scheme, blockers),
    );
    let schemas = at.key("schemas");
    for (name, schema) in &components.schemas {
        collect_schema_blockers(schemas.key(name), schema, blockers);
    }
    visit::inline_items(
        at.key("responses"),
        &components.responses,
        |at, response| {
            collect_response_blockers(at, response, blockers);
        },
    );
    visit::inline_items(
        at.key("parameters"),
        &components.parameters,
        |at, parameter| collect_parameter_blockers(at, parameter, blockers),
    );
    visit::inline_items(at.key("headers"), &components.headers, |at, header| {
        collect_header_blockers(at, header, blockers);
    });
    collect_named_example_blockers(at, Some(&components.examples), blockers);
    visit::inline_items(
        at.key("requestBodies"),
        &components.request_bodies,
        |at, body| collect_content_blockers(at, &body.content, blockers),
    );
    let mut walk = Blockers {
        blockers: &mut *blockers,
    };
    let path_items = at.key("pathItems");
    for (name, item) in &components.path_items {
        visit::path_item(path_items.key(name), item, &mut walk);
    }
    visit::inline_items(
        at.key("callbacks"),
        &components.callbacks,
        |at, callback| {
            visit::callback(at, callback, &mut walk);
        },
    );
    visit::inline_items(at.key("links"), &components.links, |at, link| {
        collect_link_blockers(at, link, blockers);
    });
}

/// The 3.2 constructs each Path Item and operation holds, wherever it hangs:
/// `paths`, `webhooks`, a component, or a callback expression.
///
/// The walk reaches every operation, `additionalOperations` included, so a
/// construct is reported where it lives, even under a map already reported.
struct Blockers<'b> {
    blockers: &'b mut Vec<String>,
}

impl<'doc> Visit<'doc> for Blockers<'_> {
    fn path_item(&mut self, at: At<'_>, item: &'doc PathItem, _path: Option<&'doc str>) -> bool {
        let Self { blockers } = self;
        if item.query.is_some() {
            blockers.push(at.key("query").pointer());
        }
        if !item.additional_operations.is_empty() {
            blockers.push(at.key("additionalOperations").pointer());
        }
        collect_parameters_blockers(at, &item.parameters, blockers);
        collect_servers_blockers(at, &item.servers, blockers);
        true
    }

    fn operation(&mut self, at: At<'_>, _item: &'doc PathItem, operation: &'doc Operation) {
        let Self { blockers } = self;
        collect_parameters_blockers(at, &operation.parameters, blockers);

        if let Some(RefOr::Item(body)) = &operation.request_body {
            collect_content_blockers(at.key("requestBody"), &body.content, blockers);
        }

        collect_responses_blockers(at.key("responses"), &operation.responses, blockers);
        collect_servers_blockers(at, &operation.servers, blockers);
    }
}

/// The keyed responses, each at its status key as written, *and* the
/// `default` beside them, last.
fn collect_responses_blockers(at: At<'_>, responses: &Responses, blockers: &mut Vec<String>) {
    // `entries` yields an inline `default` first; this walk lists it last.
    let default = responses.default_response.as_ref().and_then(RefOr::as_item);
    for (status, response) in responses.entries().skip(usize::from(default.is_some())) {
        collect_response_blockers(at.verbatim(status), response, blockers);
    }

    if let Some(default) = default {
        collect_response_blockers(at.key("default"), default, blockers);
    }
}

fn collect_response_blockers(at: At<'_>, response: &Response, blockers: &mut Vec<String>) {
    if response.summary.is_some() {
        blockers.push(at.key("summary").pointer());
    }

    collect_content_blockers(at, &response.content, blockers);

    visit::inline_items(at.key("headers"), &response.headers, |at, header| {
        collect_header_blockers(at, header, blockers);
    });

    visit::inline_items(at.key("links"), &response.links, |at, link| {
        collect_link_blockers(at, link, blockers);
    });
}

/// The inline entries of the `parameters` array of the object at `at`.
///
/// Each is located by its array index, which a reference before it also
/// takes up, since a JSON Pointer addresses an array by index, never by name.
fn collect_parameters_blockers(
    at: At<'_>,
    parameters: &[RefOr<Parameter>],
    blockers: &mut Vec<String>,
) {
    let at = at.key("parameters");
    for (index, parameter) in parameters.iter().enumerate() {
        if let RefOr::Item(parameter) = parameter {
            collect_parameter_blockers(at.index(index), parameter, blockers);
        }
    }
}

/// A parameter, located at the pointer the caller built for it.
///
/// Only the caller knows whether it is named by position or component key.
fn collect_parameter_blockers(at: At<'_>, parameter: &Parameter, blockers: &mut Vec<String>) {
    if parameter.location == ParameterIn::Querystring {
        blockers.push(at.pointer());
    }
    if parameter.style() == Some(Style::Cookie) {
        blockers.push(at.key("style").pointer());
    }

    if let Some((media_type, content)) = parameter.content() {
        collect_content_entry_blockers(at.key("content").key(media_type), content, blockers);
    }

    if let Some(schema) = parameter.schema() {
        collect_schema_blockers(at.key("schema"), schema, blockers);
    }

    collect_named_example_blockers(at, parameter.named_examples(), blockers);
}

fn collect_header_blockers(at: At<'_>, header: &Header, blockers: &mut Vec<String>) {
    if let Some((media_type, content)) = header.content() {
        collect_content_entry_blockers(at.key("content").key(media_type), content, blockers);
    }

    if let Some(schema) = header.schema() {
        collect_schema_blockers(at.key("schema"), schema, blockers);
    }

    collect_named_example_blockers(at, header.named_examples(), blockers);
}

/// Each entry of the `content` map of the object at `at`.
fn collect_content_blockers(
    at: At<'_>,
    content: &Map<RefOr<MediaType>>,
    blockers: &mut Vec<String>,
) {
    let at = at.key("content");
    for (media_type, entry) in content {
        collect_content_entry_blockers(at.key(media_type), entry, blockers);
    }
}

/// One `content` entry, located at the pointer the caller built for it.
///
/// A reference is skipped: every build reports one as a blocker in itself, and
/// what it names is walked where it is defined.
fn collect_content_entry_blockers(
    at: At<'_>,
    entry: &RefOr<MediaType>,
    blockers: &mut Vec<String>,
) {
    if let RefOr::Item(content) = entry {
        collect_media_type_blockers(at, content, blockers);
    }
}

fn collect_media_type_blockers(at: At<'_>, content: &MediaType, blockers: &mut Vec<String>) {
    for (field, present) in [
        ("itemSchema", content.item_schema.is_some()),
        ("prefixEncoding", content.prefix_encoding.is_some()),
        ("itemEncoding", content.item_encoding.is_some()),
    ] {
        if present {
            blockers.push(at.key(field).pointer());
        }
    }

    let encodings = at.key("encoding");
    for (property, encoding) in &content.encoding {
        collect_encoding_blockers(encodings.key(property), encoding, blockers);
    }

    collect_named_example_blockers(at, content.named_examples(), blockers);

    // Not `item_schema`: its presence is already a blocker above, so walking it
    // would name the same document twice.
    if let Some(schema) = &content.schema {
        collect_schema_blockers(at.key("schema"), schema, blockers);
    }
}

/// The Encoding Object's own three 3.2 fields, and those of the headers it
/// carries.
///
/// Nested encodings are not walked: each of these three *is* the nesting.
fn collect_encoding_blockers(at: At<'_>, encoding: &Encoding, blockers: &mut Vec<String>) {
    for (field, present) in [
        ("encoding", !encoding.encoding.is_empty()),
        ("prefixEncoding", encoding.prefix_encoding.is_some()),
        ("itemEncoding", encoding.item_encoding.is_some()),
    ] {
        if present {
            blockers.push(at.key(field).pointer());
        }
    }

    visit::inline_items(at.key("headers"), &encoding.headers, |at, header| {
        collect_header_blockers(at, header, blockers);
    });
}

/// Each inline example of a named-examples map, under the `examples` of the
/// object at `at`.
///
/// A component's `examples` section and a Parameter, Header or Media Type
/// Object's `examples` field are one shape.
fn collect_named_example_blockers(
    at: At<'_>,
    examples: Option<&Map<RefOr<Example>>>,
    blockers: &mut Vec<String>,
) {
    if let Some(examples) = examples {
        visit::inline_items(at.key("examples"), examples, |at, example| {
            collect_example_blockers(at, example, blockers);
        });
    }
}

/// The two example forms 3.2 added beside `value`.
///
/// `externalValue` is 3.1, so only a `dataValue` beside it blocks.
fn collect_example_blockers(at: At<'_>, example: &Example, blockers: &mut Vec<String>) {
    let (data, serialized) = match example.value() {
        Some(ExampleValue::External { data, .. }) => (data.is_some(), false),
        Some(ExampleValue::Data { serialized, .. }) => (true, serialized.is_some()),
        Some(ExampleValue::Serialized(_)) => (false, true),
        Some(ExampleValue::Embedded(_)) | None => (false, false),
    };

    for (field, present) in [("dataValue", data), ("serializedValue", serialized)] {
        if present {
            blockers.push(at.key(field).pointer());
        }
    }
}

/// `xml.nodeType` and `discriminator.defaultMapping`, wherever they are nested.
///
/// Reported over the serialized schema, so a subschema keyword added later
/// cannot be missed, and in the order its members serialize in. Both names
/// are matched directly beneath any `xml` or `discriminator` member, so one
/// in an unknown keyword, under an extension or in a `properties` entry
/// spelled `xml` is refused too, which is the safe way to be wrong.
///
/// Instance data is not descended into: what an [`INSTANCE_KEYWORDS`] member
/// holds is any JSON in 3.1 as well, so a holder in it is no 3.2 construct.
/// One is skipped wherever an object's members are keywords, an extension's
/// included, which errs the other way only for arbitrary JSON. A
/// [`NAME_MAPS`] entry is named, not a keyword, so one spelled like an
/// instance keyword is still a schema and still walked.
///
/// [`may_hold_blocker`] reads the typed schema first and rules out, without
/// serializing it, every schema that holds neither.
fn collect_schema_blockers(at: At<'_>, schema: &Schema, blockers: &mut Vec<String>) {
    if !may_hold_blocker(schema) {
        return;
    }
    let Ok(value) = serde_json::to_value(schema) else {
        return;
    };
    collect_schema_value_blockers(&at.pointer(), &value, Members::Keywords, blockers);
}

/// What the members of an object [`collect_schema_value_blockers`] reaches
/// are.
#[derive(Clone, Copy)]
enum Members {
    /// Keywords, as a schema's own members are; an instance keyword among
    /// them is skipped.
    Keywords,
    /// Names, as a [`NAME_MAPS`] member's are, each naming a schema.
    Names,
}

fn collect_schema_value_blockers(
    location: &str,
    value: &Value,
    members: Members,
    blockers: &mut Vec<String>,
) {
    match value {
        Value::Object(fields) => {
            for (holder, field) in HOLDERS {
                let carries = fields
                    .get(holder)
                    .and_then(Value::as_object)
                    .is_some_and(|held| held.contains_key(field));
                if carries {
                    blockers.push(format!("{location}/{holder}/{field}"));
                }
            }

            for (key, nested) in fields {
                let keyword = matches!(members, Members::Keywords);
                if keyword && INSTANCE_KEYWORDS.contains(&key.as_str()) {
                    continue;
                }
                let nested_members = if keyword && NAME_MAPS.contains(&key.as_str()) {
                    Members::Names
                } else {
                    Members::Keywords
                };
                collect_schema_value_blockers(
                    &format!("{location}/{}", pointer_token(key)),
                    nested,
                    nested_members,
                    blockers,
                );
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_schema_value_blockers(
                    &format!("{location}/{index}"),
                    item,
                    Members::Keywords,
                    blockers,
                );
            }
        }
        _ => {}
    }
}

/// Each member [`collect_schema_value_blockers`] looks beneath, with the
/// member it reports there.
const HOLDERS: [(&str, &str); 2] = [("xml", "nodeType"), ("discriminator", "defaultMapping")];

/// The Schema Object keywords whose values are instance data: JSON Schema
/// 2020-12's `default`, `const`, `enum` and `examples`, and the OpenAPI
/// `example` it deprecates.
const INSTANCE_KEYWORDS: [&str; 5] = ["default", "const", "enum", "examples", "example"];

/// The Schema Object keywords whose values map names to schemas.
const NAME_MAPS: [&str; 4] = [
    "$defs",
    "dependentSchemas",
    "properties",
    "patternProperties",
];

/// Whether `schema`, serialized, may hold a member of [`HOLDERS`] carrying
/// its field, at any depth.
///
/// Errs only toward `true`, so a schema it rules out is one
/// [`collect_schema_value_blockers`] reports nothing in. A Schema Object
/// serializes as its typed keywords and its unknown ones; of the typed, only
/// `xml` and `discriminator` write either field, a [`NAME_MAPS`] entry may be
/// named for a holder, and unknown keywords and extensions are arbitrary
/// JSON. Instance data is not read, since the walk skips it.
fn may_hold_blocker(schema: &Schema) -> bool {
    let Schema::Object(object) = schema else {
        return false;
    };

    let xml = object.xml.as_ref().is_some_and(|xml| {
        matches!(xml.node, XmlNode::Type(_))
            || xml.extensions.0.contains_key("nodeType")
            || members_may_hold(&xml.extensions.0)
    });
    let discriminator = object.discriminator.as_ref().is_some_and(|discriminator| {
        discriminator.default_mapping.is_some()
            || discriminator.extensions.0.contains_key("defaultMapping")
            || members_may_hold(&discriminator.extensions.0)
    });
    let external_docs = object
        .external_docs
        .as_ref()
        .is_some_and(|docs| members_may_hold(&docs.extensions.0));
    let named = [
        &object.defs,
        &object.dependent_schemas,
        &object.properties,
        &object.pattern_properties,
    ]
    .into_iter()
    .any(|schemas| {
        HOLDERS.into_iter().any(|(holder, field)| {
            schemas
                .get(holder)
                .and_then(Schema::as_object)
                .is_some_and(|held| held.unknown_keywords.contains_key(field))
        })
    });

    if xml || discriminator || external_docs || named || members_may_hold(&object.unknown_keywords)
    {
        return true;
    }

    let mut nested = false;
    object.for_each_subschema(At::ROOT, |_, subschema| {
        nested = nested || may_hold_blocker(subschema);
    });
    nested
}

/// Whether a JSON object with these members holds a member of [`HOLDERS`]
/// carrying its field, at any depth.
fn members_may_hold<'v>(members: impl IntoIterator<Item = (&'v String, &'v Value)>) -> bool {
    members.into_iter().any(|(key, value)| {
        HOLDERS.into_iter().any(|(holder, field)| {
            key == holder
                && value
                    .as_object()
                    .is_some_and(|held| held.contains_key(field))
        }) || value_may_hold(value)
    })
}

/// The same for one JSON value.
fn value_may_hold(value: &Value) -> bool {
    match value {
        Value::Object(fields) => members_may_hold(fields),
        Value::Array(items) => items.iter().any(value_may_hold),
        _ => false,
    }
}
