//! Property tests over the reachable document model.
//!
//! The generators draw names, keys and text from small pools rather than from
//! `any::<String>()`. Collisions are the point: a duplicated `operationId`, a
//! tag that is its own parent, a parameter named after a template variable that
//! is not there, are all rules the validator has to survive, and a uniform
//! string generator would never produce one.
//!
//! Everything a document can hold is generated, including values the model
//! considers invalid — the point of the validation property is that no
//! document, however malformed, makes the checker panic or diverge.

use kynos_openapi::{
    Callback, Components, Contact, Discriminator, Document, Encoding, Example, Extensions,
    ExternalDocumentation, Header, Info, License, Link, Map, MediaType, Method, OAuthFlow,
    OAuthFlows, Operation, Parameter, ParameterIn, PathItem, PathTemplate, Paths, Ref, RefOr,
    RequestBody, Response, Responses, Schema, SchemaObject, SecurityRequirement, SecurityScheme,
    Server, ServerVariable, Severity, SpecError, SpecVersion, Style, Tag, Violation, Xml,
    annotation::{
        NOT_AUTHORITATIVE_ANNOTATION, OPAQUE_OPERATION_ANNOTATION, OPAQUE_ROUTES_ANNOTATION,
    },
    emit::downgrade::three_two_only_constructs,
    model::schema::types::{SchemaType, TypeSet},
    validate::Validator,
};
use proptest::{prelude::*, sample::select};
use serde_json::{Value, json};

// --- Pools ---------------------------------------------------------------

const TEXTS: &[&str] = &["", "Orders", "a b", "héllo", "line\nbreak", "\"quoted\""];
const URLS: &[&str] = &[
    "https://api.example.com",
    "/relative",
    "{region}.example.com",
];
const COMPONENT_KEYS: &[&str] = &["User", "Order.v1", "not a name", "x_1", ""];
const SCHEME_NAMES: &[&str] = &["Bearer", "ApiKey", "Undeclared"];
const TAG_NAMES: &[&str] = &["orders", "users", "a"];
const OPERATION_IDS: &[&str] = &["getUser", "listUsers"];
const PARAMETER_NAMES: &[&str] = &["id0", "id", "Accept", "x"];
const MEDIA_TYPE_NAMES: &[&str] = &["application/json", "text/plain", "multipart/mixed"];
const STATUS_KEYS: &[&str] = &["200", "404", "2XX", "5XX"];
const EXTENSION_KEYS: &[&str] = &["x-a", "x-b", "x-oai-reserved", "not-prefixed"];
// The Responses Object rejects any key that is neither `default`, a status
// pattern, nor `x-` prefixed, so its extensions cannot be drawn from the
// general pool and still survive a round trip.
const RESPONSES_EXTENSION_KEYS: &[&str] = &["x-a", "x-b"];
const KEYWORD_KEYS: &[&str] = &["x-kynos-unchecked", "x-note", "customKeyword"];
const CALLBACK_KEYS: &[&str] = &["{$request.body#/url}", "onData"];
const OPENAPI_VERSIONS: &[&str] = &["3.1.2", "3.2.0", "3.0.4", "nonsense"];
const NUMBERS: &[f64] = &[0.0, 1.0, -2.5, 1e6];
const COUNTS: &[u64] = &[0, 1, 64];

const SCHEMA_TYPES: &[SchemaType] = &[
    SchemaType::Null,
    SchemaType::Boolean,
    SchemaType::Object,
    SchemaType::Array,
    SchemaType::Number,
    SchemaType::String,
    SchemaType::Integer,
];

const STYLES: &[Style] = &[
    Style::Matrix,
    Style::Label,
    Style::Simple,
    Style::Form,
    Style::SpaceDelimited,
    Style::PipeDelimited,
    Style::DeepObject,
    #[cfg(feature = "openapi32")]
    Style::Cookie,
];

const LOCATIONS: &[ParameterIn] = &[
    ParameterIn::Query,
    ParameterIn::Header,
    ParameterIn::Path,
    ParameterIn::Cookie,
    #[cfg(feature = "openapi32")]
    ParameterIn::Querystring,
];

const VERSIONS: &[SpecVersion] = &[
    SpecVersion::V3_1,
    #[cfg(feature = "openapi32")]
    SpecVersion::V3_2,
];

const ADDITIONAL_METHODS: &[&str] = &["LINK", "PURGE"];

// --- Leaf strategies -----------------------------------------------------

fn arb_text() -> impl Strategy<Value = String> {
    select(TEXTS).prop_map(str::to_owned)
}

fn arb_opt_text() -> impl Strategy<Value = Option<String>> {
    prop::option::of(arb_text())
}

fn arb_url() -> impl Strategy<Value = String> {
    select(URLS).prop_map(str::to_owned)
}

fn arb_flag() -> impl Strategy<Value = Option<bool>> {
    prop::option::of(any::<bool>())
}

fn arb_number() -> impl Strategy<Value = Option<f64>> {
    prop::option::of(select(NUMBERS))
}

fn arb_count() -> impl Strategy<Value = Option<u64>> {
    prop::option::of(select(COUNTS))
}

/// A map whose keys are drawn from `keys`, so that collisions are frequent.
fn arb_map<S: Strategy + 'static>(
    keys: &'static [&'static str],
    values: S,
    max: usize,
) -> BoxedStrategy<Map<S::Value>> {
    prop::collection::vec((select(keys), values), 0..=max)
        .prop_map(|entries| {
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect()
        })
        .boxed()
}

fn arb_json() -> BoxedStrategy<Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        select(&[-1i64, 0, 7][..]).prop_map(|number| Value::Number(number.into())),
        arb_text().prop_map(Value::String),
    ];
    leaf.prop_recursive(1, 4, 2, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..=2).prop_map(Value::Array),
            prop::collection::vec((select(RESPONSES_EXTENSION_KEYS), inner), 0..=2).prop_map(
                |entries| Value::Object(
                    entries
                        .into_iter()
                        .map(|(key, value)| (key.to_owned(), value))
                        .collect()
                )
            ),
        ]
    })
    .boxed()
}

/// A JSON value that is never `null`.
///
/// Every `Option<Value>` field in the model conflates an absent field with a
/// present `null`: `Some(Value::Null)` is written as `null`, and `null` is read
/// back as `None`. Nulls nested inside an array or object are unaffected, so
/// only the outermost value is constrained here.
fn arb_present_json() -> BoxedStrategy<Value> {
    prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        select(&[-1i64, 0, 7][..]).prop_map(|number| Value::Number(number.into())),
        arb_text().prop_map(Value::String),
        prop::collection::vec(arb_json(), 0..=2).prop_map(Value::Array),
    ]
    .boxed()
}

fn arb_extensions(keys: &'static [&'static str]) -> BoxedStrategy<Extensions> {
    arb_map(keys, arb_json(), 2).prop_map(Extensions).boxed()
}

/// Values seen under `x-kynos-opaque`, well-formed and otherwise.
fn arb_opaque_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(json!({"reasons": ["untyped-layer"]})),
        Just(json!({"reasons": ["untyped-layer", "a-reason-from-the-future"], "note": "n"})),
        Just(json!({"reasons": []})),
        Just(json!("not an object")),
        Just(json!({"reasons": 7})),
    ]
}

/// Values seen under `x-kynos-opaque-routes`, well-formed and otherwise.
fn arb_routes_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(json!([])),
        Just(json!([{"pattern": "/files/{*rest}", "reason": "untyped-route"}])),
        Just(json!([{
            "pattern": "/a",
            "prefix": "/a",
            "methods": ["GET"],
            "reason": "a-reason-from-the-future",
            "note": "n",
        }])),
        Just(json!({"pattern": "/x"})),
        Just(json!([{"reason": "untyped-route"}])),
    ]
}

// --- Path templates ------------------------------------------------------

/// A `pchar` run, the only thing a literal part of a template may hold.
const LITERALS: &[&str] = &[
    "users",
    "v1",
    "a-b",
    "x.y",
    "%2F",
    "%aB",
    "!$&'()*+,;=",
    ":@",
    "~_",
    "9",
];
const VARIABLE_STEMS: &[&str] = &["id", "postId", "name", "x"];

#[derive(Clone, Debug)]
enum Part {
    Literal(&'static str),
    Variable(&'static str),
}

/// A generated template together with the answers the model should give for it.
///
/// `normalized` and `variables` are built alongside `raw` rather than derived
/// from it, so they are an independent oracle rather than a second copy of the
/// parser.
#[derive(Clone, Debug)]
struct TemplateCase {
    raw: String,
    /// The same template with every variable given a different name.
    renamed: String,
    normalized: String,
    variables: Vec<String>,
}

fn build_template(
    segments: &[Vec<Part>],
    trailing_slash: bool,
    prefix: &str,
) -> (String, String, Vec<String>) {
    let mut raw = String::from("/");
    let mut normalized = String::from("/");
    let mut variables = Vec::new();

    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            raw.push('/');
            normalized.push('/');
        }
        for part in segment {
            match *part {
                Part::Literal(literal) => {
                    raw.push_str(literal);
                    normalized.push_str(literal);
                }
                Part::Variable(stem) => {
                    // The index makes the name unique within the template, and
                    // the stems carry no digits, so no two names can collide.
                    let name = format!("{prefix}{stem}{}", variables.len());
                    raw.push('{');
                    raw.push_str(&name);
                    raw.push('}');
                    normalized.push_str("{}");
                    variables.push(name);
                }
            }
        }
    }

    // The final segment is optional, so a trailing `/` is legal -- but only
    // where a segment precedes it, since `//` has an empty segment.
    if trailing_slash && !segments.is_empty() {
        raw.push('/');
        normalized.push('/');
    }

    (raw, normalized, variables)
}

fn arb_part() -> impl Strategy<Value = Part> {
    prop_oneof![
        3 => select(LITERALS).prop_map(Part::Literal),
        1 => select(VARIABLE_STEMS).prop_map(Part::Variable),
    ]
}

fn arb_template_case() -> impl Strategy<Value = TemplateCase> {
    (
        prop::collection::vec(prop::collection::vec(arb_part(), 1..=2), 0..=3),
        any::<bool>(),
    )
        .prop_map(|(segments, trailing_slash)| {
            let (raw, normalized, variables) = build_template(&segments, trailing_slash, "");
            let (renamed, ..) = build_template(&segments, trailing_slash, "z");
            TemplateCase {
                raw,
                renamed,
                normalized,
                variables,
            }
        })
}

fn arb_template() -> impl Strategy<Value = String> {
    arb_template_case().prop_map(|case| case.raw)
}

/// Templates the grammar must reject, curated and derived.
const MALFORMED_TEMPLATES: &[&str] = &[
    "", "users", "/{", "/}", "/{}", "/a}b", "/{a{b}", "/a%zz", "/a%", "/a b", "/ä", "/a|b",
    "/a\"b", "//", "/a//b", "/?", "/#", "/{a}/{a}", "/a?q=1",
];

fn arb_malformed_template() -> impl Strategy<Value = String> {
    prop_oneof![
        select(MALFORMED_TEMPLATES).prop_map(str::to_owned),
        // A query string or fragment is never part of a path.
        arb_template().prop_map(|raw| format!("{raw}?q=1")),
        arb_template().prop_map(|raw| format!("{raw}#f")),
        // Without the leading `/` nothing is a template.
        arb_template().prop_map(|raw| raw[1..].to_owned()),
        // `//` always meets two separators with nothing between them.
        arb_template().prop_map(|raw| format!("{raw}//x")),
        // Repeating a variable that is already there.
        arb_template_case().prop_map(|case| match case.variables.first() {
            Some(name) => format!("{}/{{{name}}}", case.raw),
            None => "/{dup}/{dup}".to_owned(),
        }),
    ]
}

// --- Schemas -------------------------------------------------------------

fn arb_type_set() -> impl Strategy<Value = TypeSet> {
    prop_oneof![
        select(SCHEMA_TYPES).prop_map(TypeSet::One),
        prop::collection::vec(select(SCHEMA_TYPES), 0..=2).prop_map(TypeSet::Many),
    ]
}

fn arb_xml() -> BoxedStrategy<Xml> {
    (
        arb_opt_text(),
        arb_opt_text(),
        arb_opt_text(),
        arb_flag(),
        arb_flag(),
        arb_opt_text(),
    )
        .prop_map(|(name, namespace, prefix, attribute, wrapped, node_type)| {
            #[cfg(not(feature = "openapi32"))]
            let _ = node_type;
            Xml {
                #[cfg(feature = "openapi32")]
                node_type,
                name,
                namespace,
                prefix,
                attribute,
                wrapped,
            }
        })
        .boxed()
}

fn arb_discriminator() -> BoxedStrategy<Discriminator> {
    (
        arb_text(),
        arb_map(COMPONENT_KEYS, arb_text(), 2),
        arb_opt_text(),
    )
        .prop_map(|(property_name, mapping, default_mapping)| {
            #[cfg(not(feature = "openapi32"))]
            let _ = default_mapping;
            Discriminator {
                property_name,
                mapping,
                #[cfg(feature = "openapi32")]
                default_mapping,
            }
        })
        .boxed()
}

fn arb_external_docs() -> BoxedStrategy<ExternalDocumentation> {
    (arb_opt_text(), arb_url(), arb_extensions(EXTENSION_KEYS))
        .prop_map(|(description, url, extensions)| ExternalDocumentation {
            description,
            url,
            extensions,
        })
        .boxed()
}

fn arb_leaf_schema() -> BoxedStrategy<Schema> {
    prop_oneof![
        1 => any::<bool>().prop_map(Schema::Bool),
        3 => (
            prop::option::of(arb_type_set()),
            arb_opt_text(),
            arb_opt_text(),
            arb_number(),
            arb_count(),
            arb_map(KEYWORD_KEYS, arb_json(), 2),
        )
            .prop_map(|(ty, format, reference, maximum, max_length, unknown_keywords)| {
                Schema::Object(Box::new(SchemaObject {
                    ty,
                    format,
                    reference,
                    maximum,
                    max_length,
                    unknown_keywords,
                    ..SchemaObject::default()
                }))
            }),
    ]
    .boxed()
}

fn arb_schema() -> BoxedStrategy<Schema> {
    arb_leaf_schema()
        .prop_recursive(1, 6, 2, |inner| {
            (
                prop::option::of(arb_type_set()),
                arb_map(COMPONENT_KEYS, inner.clone(), 2),
                prop::option::of(prop::collection::vec(inner.clone(), 0..=2)),
                prop::option::of(inner),
                prop::option::of(prop::collection::vec(arb_text(), 0..=2)),
                prop::option::of(arb_discriminator()),
                prop::option::of(arb_xml()),
                prop::option::of(arb_external_docs()),
            )
                .prop_map(
                    |(
                        ty,
                        properties,
                        one_of,
                        items,
                        required,
                        discriminator,
                        xml,
                        external_docs,
                    )| {
                        Schema::Object(Box::new(SchemaObject {
                            ty,
                            properties,
                            one_of,
                            items: items.map(Box::new),
                            required,
                            discriminator,
                            xml,
                            external_docs,
                            ..SchemaObject::default()
                        }))
                    },
                )
        })
        .boxed()
}

// --- References ----------------------------------------------------------

fn arb_ref() -> impl Strategy<Value = Ref> {
    (arb_text(), arb_opt_text(), arb_opt_text()).prop_map(|(location, summary, description)| Ref {
        location,
        summary,
        description,
    })
}

/// A `$ref` or the item itself, in the ratio a real description has.
fn arb_ref_or<S: Strategy + 'static>(item: S) -> BoxedStrategy<RefOr<S::Value>> {
    prop_oneof![
        1 => arb_ref().prop_map(RefOr::Ref),
        4 => item.prop_map(RefOr::Item),
    ]
    .boxed()
}

// --- Content -------------------------------------------------------------

fn arb_example() -> BoxedStrategy<Example> {
    (
        arb_opt_text(),
        arb_opt_text(),
        prop::option::of(arb_present_json()),
        prop::option::of(arb_present_json()),
        arb_opt_text(),
        arb_opt_text(),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(
                summary,
                description,
                value,
                data_value,
                serialized_value,
                external_value,
                extensions,
            )| {
                #[cfg(not(feature = "openapi32"))]
                let _ = (data_value, serialized_value);
                Example {
                    summary,
                    description,
                    value,
                    #[cfg(feature = "openapi32")]
                    data_value,
                    #[cfg(feature = "openapi32")]
                    serialized_value,
                    external_value,
                    extensions,
                }
            },
        )
        .boxed()
}

fn arb_header() -> BoxedStrategy<Header> {
    (
        arb_opt_text(),
        arb_flag(),
        arb_flag(),
        prop::option::of(select(STYLES)),
        arb_flag(),
        prop::option::of(arb_schema()),
        prop::option::of(arb_present_json()),
        arb_map(COMPONENT_KEYS, arb_ref_or(arb_example()), 1),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(
                description,
                required,
                deprecated,
                style,
                explode,
                schema,
                example,
                examples,
                extensions,
            )| Header {
                description,
                required,
                deprecated,
                style,
                explode,
                schema,
                example,
                examples,
                content: Map::new(),
                extensions,
            },
        )
        .boxed()
}

fn arb_encoding() -> BoxedStrategy<Encoding> {
    (
        arb_opt_text(),
        arb_map(PARAMETER_NAMES, arb_ref_or(arb_header()), 1),
        prop::option::of(select(STYLES)),
        arb_flag(),
        arb_flag(),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(content_type, headers, style, explode, allow_reserved, extensions)| Encoding {
                content_type,
                headers,
                style,
                explode,
                allow_reserved,
                // The nested 3.2 encoding fields are left alone: they recurse,
                // and nothing else here depends on their shape.
                #[cfg(feature = "openapi32")]
                encoding: Map::new(),
                #[cfg(feature = "openapi32")]
                prefix_encoding: None,
                #[cfg(feature = "openapi32")]
                item_encoding: None,
                extensions,
            },
        )
        .boxed()
}

fn arb_media_type() -> BoxedStrategy<MediaType> {
    (
        prop::option::of(arb_schema()),
        prop::option::of(arb_schema()),
        prop::option::of(arb_present_json()),
        arb_map(COMPONENT_KEYS, arb_ref_or(arb_example()), 1),
        arb_map(PARAMETER_NAMES, arb_encoding(), 1),
        prop::option::of(prop::collection::vec(arb_encoding(), 0..=1)),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(schema, item_schema, example, examples, encoding, prefix_encoding, extensions)| {
                #[cfg(not(feature = "openapi32"))]
                let _ = (item_schema, prefix_encoding);
                MediaType {
                    schema,
                    #[cfg(feature = "openapi32")]
                    item_schema,
                    example,
                    examples,
                    encoding,
                    #[cfg(feature = "openapi32")]
                    prefix_encoding,
                    #[cfg(feature = "openapi32")]
                    item_encoding: None,
                    extensions,
                }
            },
        )
        .boxed()
}

fn arb_content() -> BoxedStrategy<Map<MediaType>> {
    arb_map(MEDIA_TYPE_NAMES, arb_media_type(), 2).boxed()
}

fn arb_request_body() -> BoxedStrategy<RequestBody> {
    (
        arb_opt_text(),
        arb_content(),
        arb_flag(),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(|(description, content, required, extensions)| RequestBody {
            description,
            content,
            required,
            extensions,
        })
        .boxed()
}

fn arb_link() -> BoxedStrategy<Link> {
    (
        arb_opt_text(),
        prop::option::of(select(OPERATION_IDS).prop_map(str::to_owned)),
        arb_map(PARAMETER_NAMES, arb_json(), 2),
        prop::option::of(arb_present_json()),
        arb_opt_text(),
        prop::option::of(arb_server()),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(
                operation_ref,
                operation_id,
                parameters,
                request_body,
                description,
                server,
                extensions,
            )| Link {
                operation_ref,
                operation_id,
                parameters,
                request_body,
                description,
                server,
                extensions,
            },
        )
        .boxed()
}

fn arb_response() -> BoxedStrategy<Response> {
    (
        arb_opt_text(),
        arb_text(),
        arb_map(PARAMETER_NAMES, arb_ref_or(arb_header()), 2),
        arb_content(),
        arb_map(COMPONENT_KEYS, arb_ref_or(arb_link()), 1),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(summary, description, headers, content, links, extensions)| {
                #[cfg(not(feature = "openapi32"))]
                let _ = summary;
                Response {
                    #[cfg(feature = "openapi32")]
                    summary,
                    description,
                    headers,
                    content,
                    links,
                    extensions,
                }
            },
        )
        .boxed()
}

fn arb_responses() -> BoxedStrategy<Responses> {
    (
        prop::option::of(arb_ref_or(arb_response())),
        arb_map(STATUS_KEYS, arb_ref_or(arb_response()), 2),
        arb_extensions(RESPONSES_EXTENSION_KEYS),
    )
        .prop_map(|(default_response, responses, extensions)| {
            // A Responses Object holding nothing but extensions is skipped
            // whole on serialization, so those extensions would not survive
            // the round trip. Only that emptiness is generated away.
            let empty = default_response.is_none() && responses.is_empty();
            Responses {
                default_response,
                responses,
                extensions: if empty { Extensions::new() } else { extensions },
            }
        })
        .boxed()
}

// --- Parameters ----------------------------------------------------------

fn arb_parameter() -> BoxedStrategy<Parameter> {
    (
        select(PARAMETER_NAMES).prop_map(str::to_owned),
        select(LOCATIONS),
        arb_opt_text(),
        (arb_flag(), arb_flag(), arb_flag(), arb_flag(), arb_flag()),
        prop::option::of(select(STYLES)),
        prop::option::of(arb_schema()),
        prop::option::of(arb_present_json()),
        (
            arb_map(COMPONENT_KEYS, arb_ref_or(arb_example()), 1),
            arb_content(),
            arb_extensions(EXTENSION_KEYS),
        ),
    )
        .prop_map(
            |(
                name,
                location,
                description,
                (required, deprecated, allow_empty_value, explode, allow_reserved),
                style,
                schema,
                example,
                (examples, content, extensions),
            )| Parameter {
                name,
                location,
                description,
                required,
                deprecated,
                allow_empty_value,
                style,
                explode,
                allow_reserved,
                schema,
                example,
                examples,
                content,
                extensions,
            },
        )
        .boxed()
}

fn arb_parameters() -> BoxedStrategy<Vec<RefOr<Parameter>>> {
    prop::collection::vec(arb_ref_or(arb_parameter()), 0..=2).boxed()
}

// --- Servers, tags, security --------------------------------------------

fn arb_server_variable() -> BoxedStrategy<ServerVariable> {
    (
        prop::option::of(prop::collection::vec(arb_text(), 0..=2)),
        arb_text(),
        arb_opt_text(),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(enumeration, default_value, description, extensions)| ServerVariable {
                enumeration,
                default_value,
                description,
                extensions,
            },
        )
        .boxed()
}

fn arb_server() -> BoxedStrategy<Server> {
    (
        arb_url(),
        arb_opt_text(),
        arb_opt_text(),
        arb_map(VARIABLE_STEMS, arb_server_variable(), 2),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(|(url, name, description, variables, extensions)| {
            #[cfg(not(feature = "openapi32"))]
            let _ = name;
            Server {
                url,
                #[cfg(feature = "openapi32")]
                name,
                description,
                variables,
                extensions,
            }
        })
        .boxed()
}

fn arb_tag() -> BoxedStrategy<Tag> {
    (
        select(TAG_NAMES).prop_map(str::to_owned),
        arb_opt_text(),
        arb_opt_text(),
        prop::option::of(select(TAG_NAMES).prop_map(str::to_owned)),
        arb_opt_text(),
        prop::option::of(arb_external_docs()),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(name, summary, description, parent, kind, external_docs, extensions)| {
                #[cfg(not(feature = "openapi32"))]
                let _ = (summary, parent, kind);
                Tag {
                    name,
                    #[cfg(feature = "openapi32")]
                    summary,
                    description,
                    #[cfg(feature = "openapi32")]
                    parent,
                    #[cfg(feature = "openapi32")]
                    kind,
                    external_docs,
                    extensions,
                }
            },
        )
        .boxed()
}

fn arb_oauth_flow() -> BoxedStrategy<OAuthFlow> {
    (
        arb_opt_text(),
        arb_opt_text(),
        arb_opt_text(),
        arb_opt_text(),
        arb_map(TAG_NAMES, arb_text(), 2),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(
                authorization_url,
                token_url,
                device_authorization_url,
                refresh_url,
                scopes,
                extensions,
            )| {
                #[cfg(not(feature = "openapi32"))]
                let _ = device_authorization_url;
                OAuthFlow {
                    authorization_url,
                    token_url,
                    #[cfg(feature = "openapi32")]
                    device_authorization_url,
                    refresh_url,
                    scopes,
                    extensions,
                }
            },
        )
        .boxed()
}

fn arb_oauth_flows() -> BoxedStrategy<OAuthFlows> {
    (
        prop::option::of(arb_oauth_flow()),
        prop::option::of(arb_oauth_flow()),
        prop::option::of(arb_oauth_flow()),
        prop::option::of(arb_oauth_flow()),
        prop::option::of(arb_oauth_flow()),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(
                implicit,
                password,
                client_credentials,
                authorization_code,
                device_authorization,
                extensions,
            )| {
                #[cfg(not(feature = "openapi32"))]
                let _ = device_authorization;
                OAuthFlows {
                    implicit,
                    password,
                    client_credentials,
                    authorization_code,
                    #[cfg(feature = "openapi32")]
                    device_authorization,
                    extensions,
                }
            },
        )
        .boxed()
}

/// The description, deprecation flag and extensions every scheme carries.
fn arb_scheme_common() -> BoxedStrategy<(Option<String>, Option<bool>, Extensions)> {
    (arb_opt_text(), arb_flag(), arb_extensions(EXTENSION_KEYS)).boxed()
}

fn arb_security_scheme() -> BoxedStrategy<SecurityScheme> {
    prop_oneof![
        (arb_text(), select(LOCATIONS), arb_scheme_common()).prop_map(
            |(name, location, (description, deprecated, extensions))| {
                #[cfg(not(feature = "openapi32"))]
                let _ = deprecated;
                SecurityScheme::ApiKey {
                    name,
                    location,
                    description,
                    #[cfg(feature = "openapi32")]
                    deprecated,
                    extensions,
                }
            }
        ),
        (arb_text(), arb_opt_text(), arb_scheme_common()).prop_map(
            |(scheme, bearer_format, (description, deprecated, extensions))| {
                #[cfg(not(feature = "openapi32"))]
                let _ = deprecated;
                SecurityScheme::Http {
                    scheme,
                    bearer_format,
                    description,
                    #[cfg(feature = "openapi32")]
                    deprecated,
                    extensions,
                }
            }
        ),
        arb_scheme_common().prop_map(|(description, deprecated, extensions)| {
            #[cfg(not(feature = "openapi32"))]
            let _ = deprecated;
            SecurityScheme::MutualTls {
                description,
                #[cfg(feature = "openapi32")]
                deprecated,
                extensions,
            }
        }),
        (arb_oauth_flows(), arb_opt_text(), arb_scheme_common()).prop_map(
            |(flows, oauth2_metadata_url, (description, deprecated, extensions))| {
                #[cfg(not(feature = "openapi32"))]
                let _ = (oauth2_metadata_url, deprecated);
                SecurityScheme::OAuth2 {
                    flows: Box::new(flows),
                    #[cfg(feature = "openapi32")]
                    oauth2_metadata_url,
                    description,
                    #[cfg(feature = "openapi32")]
                    deprecated,
                    extensions,
                }
            }
        ),
        (arb_url(), arb_scheme_common()).prop_map(
            |(open_id_connect_url, (description, deprecated, extensions))| {
                #[cfg(not(feature = "openapi32"))]
                let _ = deprecated;
                SecurityScheme::OpenIdConnect {
                    open_id_connect_url,
                    description,
                    #[cfg(feature = "openapi32")]
                    deprecated,
                    extensions,
                }
            }
        ),
    ]
    .boxed()
}

fn arb_security_requirement() -> BoxedStrategy<SecurityRequirement> {
    arb_map(SCHEME_NAMES, prop::collection::vec(arb_text(), 0..=2), 2)
        .prop_map(SecurityRequirement)
        .boxed()
}

// --- Operations and path items ------------------------------------------

fn arb_operation_extensions() -> BoxedStrategy<Extensions> {
    (
        arb_extensions(EXTENSION_KEYS),
        prop::option::of(arb_opaque_value()),
    )
        .prop_map(|(mut extensions, opaque)| {
            if let Some(value) = opaque {
                extensions.insert(OPAQUE_OPERATION_ANNOTATION, value);
            }
            extensions
        })
        .boxed()
}

/// An operation; `nested` decides whether it carries callbacks, which hold
/// path items and would otherwise recurse without end.
fn arb_operation(nested: bool) -> BoxedStrategy<Operation> {
    let callbacks = if nested {
        arb_map(CALLBACK_KEYS, arb_ref_or(arb_callback()), 1).boxed()
    } else {
        Just(Map::new()).boxed()
    };

    (
        prop::collection::vec(select(TAG_NAMES).prop_map(str::to_owned), 0..=2),
        arb_opt_text(),
        arb_opt_text(),
        prop::option::of(arb_external_docs()),
        prop::option::of(select(OPERATION_IDS).prop_map(str::to_owned)),
        arb_parameters(),
        prop::option::of(arb_ref_or(arb_request_body())),
        (
            arb_responses(),
            callbacks,
            arb_flag(),
            prop::option::of(prop::collection::vec(arb_security_requirement(), 0..=2)),
            prop::collection::vec(arb_server(), 0..=1),
            arb_operation_extensions(),
        ),
    )
        .prop_map(
            |(
                tags,
                summary,
                description,
                external_docs,
                operation_id,
                parameters,
                request_body,
                (responses, callbacks, deprecated, security, servers, extensions),
            )| Operation {
                tags,
                summary,
                description,
                external_docs,
                operation_id,
                parameters,
                request_body,
                responses,
                callbacks,
                deprecated,
                security,
                servers,
                extensions,
            },
        )
        .boxed()
}

/// A callback, whose path items never carry callbacks of their own.
fn arb_callback() -> BoxedStrategy<Callback> {
    arb_map(CALLBACK_KEYS, arb_ref_or(arb_path_item(false, false)), 1)
        .prop_map(Callback)
        .boxed()
}

/// A path item.
///
/// `referenceable` is false wherever the item sits inside a `RefOr`: an item
/// that carries `$ref` serializes exactly like a Reference Object and would be
/// read back as one. `nested` is false one level down, where the callbacks and
/// additional operations that lead back here have to stop.
fn arb_path_item(referenceable: bool, nested: bool) -> BoxedStrategy<PathItem> {
    let additional = if nested {
        arb_map(ADDITIONAL_METHODS, arb_operation(false), 1).boxed()
    } else {
        Just(Map::new()).boxed()
    };

    (
        arb_opt_text(),
        arb_opt_text(),
        arb_opt_text(),
        prop::collection::vec((select(Method::all()), arb_operation(nested)), 0..=2),
        additional,
        prop::collection::vec(arb_server(), 0..=1),
        arb_parameters(),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            move |(
                reference,
                summary,
                description,
                operations,
                additional,
                servers,
                parameters,
                extensions,
            )| {
                let mut item = PathItem::new();
                if referenceable {
                    item.reference = reference;
                }
                item.summary = summary;
                item.description = description;
                for (method, operation) in operations {
                    item.set_operation(method, operation);
                }
                #[cfg(feature = "openapi32")]
                {
                    item.additional_operations = additional
                        .into_iter()
                        .map(|(method, operation)| (method, Box::new(operation)))
                        .collect();
                }
                #[cfg(not(feature = "openapi32"))]
                let _: Map<Operation> = additional;
                item.servers = servers;
                item.parameters = parameters;
                item.extensions = extensions;
                item
            },
        )
        .boxed()
}

fn arb_paths() -> BoxedStrategy<Paths> {
    let keys = prop_oneof![
        4 => arb_template(),
        1 => arb_malformed_template(),
    ];
    prop::collection::vec((keys, arb_path_item(true, true)), 0..=2)
        .prop_map(|entries| Paths(entries.into_iter().collect()))
        .boxed()
}

// --- Components and document --------------------------------------------

fn arb_components() -> BoxedStrategy<Components> {
    (
        arb_map(COMPONENT_KEYS, arb_schema(), 2),
        arb_map(COMPONENT_KEYS, arb_ref_or(arb_response()), 1),
        arb_map(COMPONENT_KEYS, arb_ref_or(arb_parameter()), 1),
        arb_map(COMPONENT_KEYS, arb_ref_or(arb_example()), 1),
        arb_map(COMPONENT_KEYS, arb_ref_or(arb_request_body()), 1),
        arb_map(COMPONENT_KEYS, arb_ref_or(arb_header()), 1),
        arb_map(SCHEME_NAMES, arb_ref_or(arb_security_scheme()), 2),
        (
            arb_map(COMPONENT_KEYS, arb_ref_or(arb_link()), 1),
            arb_map(COMPONENT_KEYS, arb_ref_or(arb_callback()), 1),
            arb_map(COMPONENT_KEYS, arb_path_item(true, false), 1),
            arb_map(COMPONENT_KEYS, arb_ref_or(arb_media_type()), 1),
            arb_extensions(EXTENSION_KEYS),
        ),
    )
        .prop_map(
            |(
                schemas,
                responses,
                parameters,
                examples,
                request_bodies,
                headers,
                security_schemes,
                (links, callbacks, path_items, media_types, extensions),
            )| {
                #[cfg(not(feature = "openapi32"))]
                let _ = media_types;
                Components {
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
                }
            },
        )
        .boxed()
}

fn arb_contact() -> BoxedStrategy<Contact> {
    (
        arb_opt_text(),
        arb_opt_text(),
        arb_opt_text(),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(|(name, url, email, extensions)| Contact {
            name,
            url,
            email,
            extensions,
        })
        .boxed()
}

fn arb_license() -> BoxedStrategy<License> {
    (
        arb_text(),
        arb_opt_text(),
        arb_opt_text(),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(|(name, identifier, url, extensions)| License {
            name,
            identifier,
            url,
            extensions,
        })
        .boxed()
}

fn arb_info() -> BoxedStrategy<Info> {
    (
        arb_text(),
        arb_opt_text(),
        arb_opt_text(),
        arb_opt_text(),
        prop::option::of(arb_contact()),
        prop::option::of(arb_license()),
        arb_text(),
        arb_extensions(EXTENSION_KEYS),
    )
        .prop_map(
            |(
                title,
                summary,
                description,
                terms_of_service,
                contact,
                license,
                version,
                extensions,
            )| Info {
                title,
                summary,
                description,
                terms_of_service,
                contact,
                license,
                version,
                extensions,
            },
        )
        .boxed()
}

fn arb_document_extensions() -> BoxedStrategy<Extensions> {
    (
        arb_extensions(EXTENSION_KEYS),
        prop::option::of(arb_routes_value()),
        any::<bool>(),
    )
        .prop_map(|(mut extensions, routes, stamped)| {
            if let Some(value) = routes {
                extensions.insert(OPAQUE_ROUTES_ANNOTATION, value);
            }
            if stamped {
                extensions.insert(NOT_AUTHORITATIVE_ANNOTATION, true);
            }
            extensions
        })
        .boxed()
}

fn arb_document() -> BoxedStrategy<Document> {
    (
        select(OPENAPI_VERSIONS).prop_map(str::to_owned),
        arb_opt_text(),
        arb_info(),
        arb_opt_text(),
        prop::collection::vec(arb_server(), 0..=2),
        arb_paths(),
        arb_map(CALLBACK_KEYS, arb_path_item(true, false), 1),
        (
            arb_components(),
            prop::collection::vec(arb_security_requirement(), 0..=2),
            prop::collection::vec(arb_tag(), 0..=3),
            prop::option::of(arb_external_docs()),
            arb_document_extensions(),
        ),
    )
        .prop_map(
            |(
                openapi,
                self_uri,
                info,
                json_schema_dialect,
                servers,
                paths,
                webhooks,
                (components, security, tags, external_docs, extensions),
            )| {
                #[cfg(not(feature = "openapi32"))]
                let _ = self_uri;
                Document {
                    openapi,
                    #[cfg(feature = "openapi32")]
                    self_uri,
                    info,
                    json_schema_dialect,
                    servers,
                    paths,
                    webhooks,
                    components,
                    security,
                    tags,
                    external_docs,
                    extensions,
                }
            },
        )
        .boxed()
}

// --- Helpers used by the properties -------------------------------------

fn to_json(document: &Document) -> String {
    document
        .to_json()
        .expect("every generated value is representable in JSON")
}

fn parse(json: &str) -> Document {
    serde_json::from_str(json).expect("what the model emits, the model reads")
}

/// Violations as a sorted multiset of their rendered form.
///
/// Two rules collect their inputs through a `HashSet`, so the order of the
/// violations they emit is not fixed; the set of them is what the caller acts
/// on.
fn rendered(violations: &[Violation]) -> Vec<String> {
    let mut rendered: Vec<String> = violations.iter().map(ToString::to_string).collect();
    rendered.sort();
    rendered
}

/// The rendered form of the error-severity violations only.
fn rendered_errors(violations: &[Violation]) -> Vec<String> {
    let mut rendered: Vec<String> = violations
        .iter()
        .filter(|violation| violation.severity == Severity::Error)
        .map(ToString::to_string)
        .collect();
    rendered.sort();
    rendered
}

// --- Properties ----------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Parsing what was emitted yields the document that was emitted.
    #[test]
    fn a_document_survives_a_json_round_trip(document in arb_document()) {
        prop_assert_eq!(parse(&to_json(&document)), document);
    }

    /// Serialization depends on nothing but the value.
    #[test]
    fn serialization_is_deterministic(document in arb_document()) {
        let json = to_json(&document);
        prop_assert_eq!(to_json(&document.clone()), json.clone());
        prop_assert_eq!(to_json(&parse(&json)), json);
    }

    /// Validation terminates and reports the same thing every time, at every
    /// specification version, for any document at all.
    #[test]
    fn validation_is_total(document in arb_document()) {
        for &version in VERSIONS {
            let validator = Validator::new(version);
            let violations = validator.validate(&document);
            prop_assert_eq!(rendered(&validator.validate(&document)), rendered(&violations));

            let errors = document.validate(version).err().unwrap_or_default();
            prop_assert_eq!(rendered(&errors), rendered_errors(&violations));

            for violation in &violations {
                prop_assert!(violation.location.starts_with('#'));
            }
        }
    }

    /// Emitting as 3.1 fails exactly when a 3.2-only construct is in the way,
    /// and emitting at a fixed version is idempotent.
    #[test]
    fn emitting_refuses_a_lossy_downgrade(document in arb_document()) {
        let blockers = three_two_only_constructs(&document);
        match document.emit(SpecVersion::V3_1) {
            Ok(emitted) => {
                prop_assert!(blockers.is_empty());
                prop_assert_eq!(&emitted.openapi, "3.1.2");
                prop_assert_eq!(emitted.emit(SpecVersion::V3_1).ok(), Some(emitted.clone()));
                prop_assert_eq!(three_two_only_constructs(&emitted), blockers);
            }
            Err(error) => {
                prop_assert!(!blockers.is_empty());
                prop_assert_eq!(error, SpecError::RequiresV3_2 { blockers });
            }
        }
    }

    /// A document is emittable as the version it already declares.
    #[cfg(feature = "openapi32")]
    #[test]
    fn emitting_as_the_newer_version_always_succeeds(document in arb_document()) {
        let emitted = document.emit(SpecVersion::V3_2).expect("3.2 expresses everything");
        prop_assert_eq!(&emitted.openapi, "3.2.0");
        prop_assert_eq!(emitted.emit(SpecVersion::V3_2).ok(), Some(emitted.clone()));
        prop_assert_eq!(emitted.spec_version(), Some(SpecVersion::V3_2));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Every template the generator builds parses, and parsing answers exactly
    /// what the generator put in.
    #[test]
    fn a_well_formed_template_parses(case in arb_template_case()) {
        let parsed = PathTemplate::parse(case.raw.clone());
        prop_assert!(parsed.is_ok(), "`{}` did not parse: {:?}", case.raw, parsed);
        let template = parsed.expect("just checked");

        prop_assert_eq!(template.as_str(), case.raw.as_str());
        prop_assert_eq!(template.variables(), case.variables.as_slice());
        prop_assert_eq!(template.normalized(), case.normalized);
        prop_assert_eq!(template.to_string(), case.raw);
    }

    /// Parsing is idempotent: what a template renders as parses back to it.
    #[test]
    fn parsing_a_template_is_idempotent(case in arb_template_case()) {
        let template = PathTemplate::parse(case.raw).expect("well formed");
        let reparsed = PathTemplate::parse(template.as_str()).expect("still well formed");

        prop_assert_eq!(&reparsed, &template);
        prop_assert_eq!(reparsed.normalized(), template.normalized());
        prop_assert_eq!(
            serde_json::from_str::<PathTemplate>(
                &serde_json::to_string(&template).expect("serializable")
            ).expect("readable"),
            template
        );
    }

    /// Two templates are the same path exactly when their normalized forms
    /// agree -- renaming every variable changes the template but not the path.
    #[test]
    fn normalization_identifies_paths_up_to_variable_names(case in arb_template_case()) {
        let template = PathTemplate::parse(case.raw).expect("well formed");
        let renamed = PathTemplate::parse(case.renamed).expect("well formed");

        prop_assert_eq!(renamed.normalized(), template.normalized());
        prop_assert_eq!(renamed.variables().len(), template.variables().len());
        prop_assert_eq!(renamed == template, template.variables().is_empty());
        // With no variables there is nothing to normalize away.
        if template.variables().is_empty() {
            prop_assert_eq!(template.normalized(), template.as_str());
        }
    }

    /// Two templates whose normalized forms differ are different paths, and
    /// vice versa.
    #[test]
    fn normalized_forms_agree_exactly_for_the_same_path(
        left in arb_template_case(),
        right in arb_template_case(),
    ) {
        let left_template = PathTemplate::parse(left.raw).expect("well formed");
        let right_template = PathTemplate::parse(right.raw).expect("well formed");

        prop_assert_eq!(
            left_template.normalized() == right_template.normalized(),
            left.normalized == right.normalized
        );
    }

    /// Nothing the generator deliberately malforms is accepted.
    #[test]
    fn a_malformed_template_is_rejected(raw in arb_malformed_template()) {
        prop_assert!(PathTemplate::parse(raw.clone()).is_err(), "`{}` parsed", raw);
    }

    /// A prefix concatenates, or fails for a reason the grammar states.
    #[test]
    fn prefixing_produces_a_template_or_an_error(
        case in arb_template_case(),
        prefix in arb_template(),
    ) {
        let template = PathTemplate::parse(case.raw).expect("well formed");
        if let Ok(prefixed) = template.with_prefix(&prefix) {
            prop_assert!(prefixed.as_str().ends_with(template.as_str()));
            // A prefix contributes its own variables, ahead of these.
            prop_assert!(prefixed.variables().ends_with(template.variables()));
        }
    }
}
