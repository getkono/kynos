//! Schema rules: the annotation that marks a schema as deliberately
//! unconstrained, wherever a document holds one.
//!
//! Every schema a document writes is visited once, where it is written: the
//! reusable schemas, each parameter's and header's own schema, and each media
//! type's `schema` (and 3.2 `itemSchema`) under an operation, a reusable
//! component or a callback — then every subschema below each of them. A
//! `$ref` is never followed. The definition it names is visited where it is
//! written, so a component named twice is reported once, and no cycle guard is
//! needed over what is an owned tree.

use crate::{
    Map,
    annotation::UNCHECKED_SCHEMA_ANNOTATION,
    model::{
        body::{RequestBody, encoding::Encoding, media_type::MediaType},
        callback::Callback,
        document::Document,
        parameter::{
            Parameter, ParameterShape,
            header::{Header, HeaderShape},
        },
        paths::{item::PathItem, operation::Operation},
        reference::RefOr,
        response::Response,
        schema::{Schema, object::SchemaObject},
    },
    validate::violation::{SpecError, Violation, pointer_token},
};

pub(in crate::validate) fn check_unchecked_schemas(
    document: &Document,
    violations: &mut Vec<Violation>,
) {
    for (raw, item) in &document.paths.items {
        check_path_item(&format!("#/paths/{}", pointer_token(raw)), item, violations);
    }
    for (name, item) in &document.webhooks {
        check_path_item(
            &format!("#/webhooks/{}", pointer_token(name)),
            item,
            violations,
        );
    }

    let components = &document.components;
    for (name, schema) in &components.schemas {
        check_schema(
            &format!("#/components/schemas/{}", pointer_token(name)),
            schema,
            violations,
        );
    }
    for (location, response) in items("#/components/responses", &components.responses) {
        check_response(&location, response, violations);
    }
    for (location, parameter) in items("#/components/parameters", &components.parameters) {
        check_parameter(&location, parameter, violations);
    }
    for (location, body) in items("#/components/requestBodies", &components.request_bodies) {
        check_request_body(&location, body, violations);
    }
    for (location, header) in items("#/components/headers", &components.headers) {
        check_header(&location, header, violations);
    }
    for (location, callback) in items("#/components/callbacks", &components.callbacks) {
        check_callback(&location, callback, violations);
    }
    for (name, item) in &components.path_items {
        check_path_item(
            &format!("#/components/pathItems/{}", pointer_token(name)),
            item,
            violations,
        );
    }
    #[cfg(feature = "openapi32")]
    for (location, media_type) in items("#/components/mediaTypes", &components.media_types) {
        check_media_type(&location, media_type, violations);
    }
}

/// Each inline item of a `RefOr` map, at the pointer it is written at.
fn items<'doc, T>(
    section: &'doc str,
    entries: &'doc Map<RefOr<T>>,
) -> impl Iterator<Item = (String, &'doc T)> {
    entries.iter().filter_map(move |(name, entry)| {
        entry
            .as_item()
            .map(|item| (format!("{section}/{}", pointer_token(name)), item))
    })
}

fn check_path_item(location: &str, item: &PathItem, violations: &mut Vec<Violation>) {
    check_parameters(location, &item.parameters, violations);

    for (method, operation) in item.operations() {
        check_operation(
            &format!("{location}/{}", method.as_wire_str().to_lowercase()),
            operation,
            violations,
        );
    }
    #[cfg(feature = "openapi32")]
    for (method, operation) in &item.additional_operations {
        check_operation(
            &format!("{location}/additionalOperations/{}", pointer_token(method)),
            operation,
            violations,
        );
    }
}

fn check_operation(location: &str, operation: &Operation, violations: &mut Vec<Violation>) {
    check_parameters(location, &operation.parameters, violations);

    if let Some(RefOr::Item(body)) = &operation.request_body {
        check_request_body(&format!("{location}/requestBody"), body, violations);
    }

    if let Some(RefOr::Item(response)) = &operation.responses.default_response {
        check_response(
            &format!("{location}/responses/default"),
            response,
            violations,
        );
    }
    for (location, response) in items(
        &format!("{location}/responses"),
        &operation.responses.responses,
    ) {
        check_response(&location, response, violations);
    }

    for (location, callback) in items(&format!("{location}/callbacks"), &operation.callbacks) {
        check_callback(&location, callback, violations);
    }
}

fn check_callback(location: &str, callback: &Callback, violations: &mut Vec<Violation>) {
    for (location, item) in items(location, &callback.items) {
        check_path_item(&location, item, violations);
    }
}

fn check_parameters(
    location: &str,
    parameters: &[RefOr<Parameter>],
    violations: &mut Vec<Violation>,
) {
    for (index, parameter) in parameters.iter().enumerate() {
        if let RefOr::Item(parameter) = parameter {
            check_parameter(
                &format!("{location}/parameters/{index}"),
                parameter,
                violations,
            );
        }
    }
}

fn check_parameter(location: &str, parameter: &Parameter, violations: &mut Vec<Violation>) {
    match parameter.shape() {
        ParameterShape::Schema { schema, .. } => {
            check_schema(&format!("{location}/schema"), schema, violations);
        }
        ParameterShape::Content { media_type, value } => check_media_type(
            &format!("{location}/content/{}", pointer_token(media_type)),
            value,
            violations,
        ),
    }
}

fn check_header(location: &str, header: &Header, violations: &mut Vec<Violation>) {
    match header.shape() {
        HeaderShape::Schema { schema, .. } => {
            check_schema(&format!("{location}/schema"), schema, violations);
        }
        HeaderShape::Content { media_type, value } => check_media_type(
            &format!("{location}/content/{}", pointer_token(media_type)),
            value,
            violations,
        ),
    }
}

fn check_request_body(location: &str, body: &RequestBody, violations: &mut Vec<Violation>) {
    check_content(location, &body.content, violations);
}

fn check_response(location: &str, response: &Response, violations: &mut Vec<Violation>) {
    for (location, header) in items(&format!("{location}/headers"), &response.headers) {
        check_header(&location, header, violations);
    }
    check_content(location, &response.content, violations);
}

fn check_content(location: &str, content: &Map<MediaType>, violations: &mut Vec<Violation>) {
    for (name, media_type) in content {
        check_media_type(
            &format!("{location}/content/{}", pointer_token(name)),
            media_type,
            violations,
        );
    }
}

fn check_media_type(location: &str, media_type: &MediaType, violations: &mut Vec<Violation>) {
    if let Some(schema) = &media_type.schema {
        check_payload(&format!("{location}/schema"), schema, violations);
    }
    #[cfg(feature = "openapi32")]
    if let Some(schema) = &media_type.item_schema {
        check_payload(&format!("{location}/itemSchema"), schema, violations);
    }

    for (property, encoding) in &media_type.encoding {
        check_encoding(
            &format!("{location}/encoding/{}", pointer_token(property)),
            encoding,
            violations,
        );
    }
    #[cfg(feature = "openapi32")]
    {
        for (index, encoding) in media_type.prefix_encoding.iter().flatten().enumerate() {
            check_encoding(
                &format!("{location}/prefixEncoding/{index}"),
                encoding,
                violations,
            );
        }
        if let Some(encoding) = &media_type.item_encoding {
            check_encoding(&format!("{location}/itemEncoding"), encoding, violations);
        }
    }
}

/// The headers of an encoded part, and under 3.2 the parts nested inside it.
fn check_encoding(location: &str, encoding: &Encoding, violations: &mut Vec<Violation>) {
    for (location, header) in items(&format!("{location}/headers"), &encoding.headers) {
        check_header(&location, header, violations);
    }
    #[cfg(feature = "openapi32")]
    {
        for (property, nested) in &encoding.encoding {
            check_encoding(
                &format!("{location}/encoding/{}", pointer_token(property)),
                nested,
                violations,
            );
        }
        for (index, nested) in encoding.prefix_encoding.iter().flatten().enumerate() {
            check_encoding(
                &format!("{location}/prefixEncoding/{index}"),
                nested,
                violations,
            );
        }
        if let Some(nested) = &encoding.item_encoding {
            check_encoding(&format!("{location}/itemEncoding"), nested, violations);
        }
    }
}

/// A media type's own schema, the one position where `true` is the payload.
///
/// Below it `true` is an ordinary keyword value: the problem document Kynos
/// emits for every JSON body carries `additionalProperties: true`, and
/// reporting that would refuse every such router under
/// `deny_unchecked_schemas`.
fn check_payload(location: &str, schema: &Schema, violations: &mut Vec<Violation>) {
    if matches!(schema, Schema::Bool(true)) {
        violations.push(Violation::warning(location, SpecError::UncheckedSchema));
    }
    check_schema(location, schema, violations);
}

/// One schema carrying the annotation, then every subschema below it.
fn check_schema(location: &str, schema: &Schema, violations: &mut Vec<Violation>) {
    let Schema::Object(object) = schema else {
        return;
    };
    if object
        .unknown_keywords
        .contains_key(UNCHECKED_SCHEMA_ANNOTATION)
    {
        violations.push(Violation::warning(location, SpecError::UncheckedSchema));
    }
    check_subschemas(location, object, violations);
}

/// Every JSON Schema 2020-12 keyword whose value holds a schema, in the order
/// [`SchemaObject`] declares them.
fn check_subschemas(location: &str, object: &SchemaObject, violations: &mut Vec<Violation>) {
    let keyed = |keyword: &str, schemas: &Map<Schema>, violations: &mut Vec<Violation>| {
        for (key, schema) in schemas {
            check_schema(
                &format!("{location}/{keyword}/{}", pointer_token(key)),
                schema,
                violations,
            );
        }
    };
    let listed = |keyword: &str, schemas: Option<&[Schema]>, violations: &mut Vec<Violation>| {
        for (index, schema) in schemas.into_iter().flatten().enumerate() {
            check_schema(&format!("{location}/{keyword}/{index}"), schema, violations);
        }
    };
    let single = |keyword: &str, schema: Option<&Schema>, violations: &mut Vec<Violation>| {
        if let Some(schema) = schema {
            check_schema(&format!("{location}/{keyword}"), schema, violations);
        }
    };

    keyed("$defs", &object.defs, violations);
    listed("allOf", object.all_of.as_deref(), violations);
    listed("anyOf", object.any_of.as_deref(), violations);
    listed("oneOf", object.one_of.as_deref(), violations);
    single("not", object.not.as_deref(), violations);
    single("if", object.if_schema.as_deref(), violations);
    single("then", object.then_schema.as_deref(), violations);
    single("else", object.else_schema.as_deref(), violations);
    keyed("dependentSchemas", &object.dependent_schemas, violations);
    listed("prefixItems", object.prefix_items.as_deref(), violations);
    single("items", object.items.as_deref(), violations);
    single("contains", object.contains.as_deref(), violations);
    keyed("properties", &object.properties, violations);
    keyed("patternProperties", &object.pattern_properties, violations);
    single(
        "additionalProperties",
        object.additional_properties.as_deref(),
        violations,
    );
    single(
        "propertyNames",
        object.property_names.as_deref(),
        violations,
    );
    single(
        "unevaluatedItems",
        object.unevaluated_items.as_deref(),
        violations,
    );
    single(
        "unevaluatedProperties",
        object.unevaluated_properties.as_deref(),
        violations,
    );
    single(
        "contentSchema",
        object.content_schema.as_deref(),
        violations,
    );
}
