//! Schema rules, wherever a document holds a schema: the annotation that
//! marks one as deliberately unconstrained, and the Discriminator Object's
//! MUSTs, which [`discriminator`](super::discriminator) reads.
//!
//! Every schema a document writes is visited once, where it is written, then
//! every subschema below it. A `$ref` is never followed, so a component is
//! reported once and no cycle guard is needed.

use crate::{
    Map,
    annotation::UNCHECKED_SCHEMA_ANNOTATION,
    model::{
        body::{RequestBody, media_type::MediaType},
        callback::Callback,
        components::Components,
        document::{Document, SpecVersion},
        parameter::{
            Parameter, ParameterShape,
            header::{Header, HeaderShape},
        },
        paths::{item::PathItem, operation::Operation},
        reference::RefOr,
        response::Response,
        schema::{Schema, object::SchemaObject},
    },
    validate::{
        rules::{content::for_each_encoding, discriminator::Discriminators},
        violation::{SpecError, Violation, pointer_token},
    },
};

/// What one walk over a document's schemas reads and gathers.
struct Walk<'doc> {
    components: &'doc Components,
    version: SpecVersion,
    violations: Vec<Violation>,
    discriminators: Discriminators,
}

pub(in crate::validate) fn check_schemas(
    document: &Document,
    version: SpecVersion,
    violations: &mut Vec<Violation>,
) {
    let mut walk = Walk {
        components: &document.components,
        version,
        violations: Vec::new(),
        discriminators: Discriminators::default(),
    };
    walk_document(document, &mut walk);

    violations.append(&mut walk.violations);
    walk.discriminators.finish(violations);
}

fn walk_document(document: &Document, walk: &mut Walk<'_>) {
    for (raw, item) in &document.paths.items {
        check_path_item(&format!("#/paths/{}", pointer_token(raw)), item, walk);
    }
    for (name, item) in &document.webhooks {
        check_path_item(&format!("#/webhooks/{}", pointer_token(name)), item, walk);
    }

    let components = &document.components;
    for (name, schema) in &components.schemas {
        check_schema(
            &format!("#/components/schemas/{}", pointer_token(name)),
            schema,
            walk,
        );
    }
    for (location, response) in items("#/components/responses", &components.responses) {
        check_response(&location, response, walk);
    }
    for (location, parameter) in items("#/components/parameters", &components.parameters) {
        check_parameter(&location, parameter, walk);
    }
    for (location, body) in items("#/components/requestBodies", &components.request_bodies) {
        check_request_body(&location, body, walk);
    }
    for (location, header) in items("#/components/headers", &components.headers) {
        check_header(&location, header, walk);
    }
    for (location, callback) in items("#/components/callbacks", &components.callbacks) {
        check_callback(&location, callback, walk);
    }
    for (name, item) in &components.path_items {
        check_path_item(
            &format!("#/components/pathItems/{}", pointer_token(name)),
            item,
            walk,
        );
    }
    #[cfg(feature = "openapi32")]
    for (location, media_type) in items("#/components/mediaTypes", &components.media_types) {
        check_media_type(&location, media_type, walk);
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

fn check_path_item(location: &str, item: &PathItem, walk: &mut Walk<'_>) {
    check_parameters(location, &item.parameters, walk);

    for (method, operation) in item.operations() {
        check_operation(
            &format!("{location}/{}", method.as_wire_str().to_lowercase()),
            operation,
            walk,
        );
    }
    #[cfg(feature = "openapi32")]
    for (method, operation) in &item.additional_operations {
        check_operation(
            &format!("{location}/additionalOperations/{}", pointer_token(method)),
            operation,
            walk,
        );
    }
}

fn check_operation(location: &str, operation: &Operation, walk: &mut Walk<'_>) {
    check_parameters(location, &operation.parameters, walk);

    if let Some(RefOr::Item(body)) = &operation.request_body {
        check_request_body(&format!("{location}/requestBody"), body, walk);
    }

    if let Some(RefOr::Item(response)) = &operation.responses.default_response {
        check_response(&format!("{location}/responses/default"), response, walk);
    }
    for (location, response) in items(
        &format!("{location}/responses"),
        &operation.responses.responses,
    ) {
        check_response(&location, response, walk);
    }

    for (location, callback) in items(&format!("{location}/callbacks"), &operation.callbacks) {
        check_callback(&location, callback, walk);
    }
}

fn check_callback(location: &str, callback: &Callback, walk: &mut Walk<'_>) {
    for (location, item) in items(location, &callback.items) {
        check_path_item(&location, item, walk);
    }
}

fn check_parameters(location: &str, parameters: &[RefOr<Parameter>], walk: &mut Walk<'_>) {
    for (index, parameter) in parameters.iter().enumerate() {
        if let RefOr::Item(parameter) = parameter {
            check_parameter(&format!("{location}/parameters/{index}"), parameter, walk);
        }
    }
}

fn check_parameter(location: &str, parameter: &Parameter, walk: &mut Walk<'_>) {
    match parameter.shape() {
        ParameterShape::Schema { schema, .. } => {
            check_schema(&format!("{location}/schema"), schema, walk);
        }
        ParameterShape::Content { media_type, value } => check_media_type(
            &format!("{location}/content/{}", pointer_token(media_type)),
            value,
            walk,
        ),
    }
}

fn check_header(location: &str, header: &Header, walk: &mut Walk<'_>) {
    match header.shape() {
        HeaderShape::Schema { schema, .. } => {
            check_schema(&format!("{location}/schema"), schema, walk);
        }
        HeaderShape::Content { media_type, value } => check_media_type(
            &format!("{location}/content/{}", pointer_token(media_type)),
            value,
            walk,
        ),
    }
}

fn check_request_body(location: &str, body: &RequestBody, walk: &mut Walk<'_>) {
    check_content(location, &body.content, walk);
}

fn check_response(location: &str, response: &Response, walk: &mut Walk<'_>) {
    for (location, header) in items(&format!("{location}/headers"), &response.headers) {
        check_header(&location, header, walk);
    }
    check_content(location, &response.content, walk);
}

fn check_content(location: &str, content: &Map<MediaType>, walk: &mut Walk<'_>) {
    for (name, media_type) in content {
        check_media_type(
            &format!("{location}/content/{}", pointer_token(name)),
            media_type,
            walk,
        );
    }
}

fn check_media_type(location: &str, media_type: &MediaType, walk: &mut Walk<'_>) {
    if let Some(schema) = &media_type.schema {
        check_payload(&format!("{location}/schema"), schema, walk);
    }
    #[cfg(feature = "openapi32")]
    if let Some(schema) = &media_type.item_schema {
        check_payload(&format!("{location}/itemSchema"), schema, walk);
    }

    // The headers of every encoded part, at every depth.
    for_each_encoding(location, media_type, &mut |location, encoding| {
        for (location, header) in items(&format!("{location}/headers"), &encoding.headers) {
            check_header(&location, header, walk);
        }
    });
}

/// A media type's own schema, the one position where `true` is the payload.
///
/// Below it `true` is an ordinary keyword value, such as the problem
/// document's `additionalProperties: true`.
fn check_payload(location: &str, schema: &Schema, walk: &mut Walk<'_>) {
    if matches!(schema, Schema::Bool(true)) {
        walk.violations
            .push(Violation::warning(location, SpecError::UncheckedSchema));
    }
    check_schema(location, schema, walk);
}

/// One schema's own rules, then every subschema below it.
fn check_schema(location: &str, schema: &Schema, walk: &mut Walk<'_>) {
    let Schema::Object(object) = schema else {
        return;
    };
    if object
        .unknown_keywords
        .contains_key(UNCHECKED_SCHEMA_ANNOTATION)
    {
        walk.violations
            .push(Violation::warning(location, SpecError::UncheckedSchema));
    }
    walk.discriminators.visit(
        location,
        object,
        walk.components,
        walk.version,
        &mut walk.violations,
    );
    check_subschemas(location, object, walk);
}

/// Every JSON Schema 2020-12 keyword whose value holds a schema, in the order
/// [`SchemaObject`] declares them.
fn check_subschemas(location: &str, object: &SchemaObject, walk: &mut Walk<'_>) {
    let keyed = |keyword: &str, schemas: &Map<Schema>, walk: &mut Walk<'_>| {
        for (key, schema) in schemas {
            check_schema(
                &format!("{location}/{keyword}/{}", pointer_token(key)),
                schema,
                walk,
            );
        }
    };
    let listed = |keyword: &str, schemas: Option<&[Schema]>, walk: &mut Walk<'_>| {
        for (index, schema) in schemas.into_iter().flatten().enumerate() {
            check_schema(&format!("{location}/{keyword}/{index}"), schema, walk);
        }
    };
    let single = |keyword: &str, schema: Option<&Schema>, walk: &mut Walk<'_>| {
        if let Some(schema) = schema {
            check_schema(&format!("{location}/{keyword}"), schema, walk);
        }
    };

    keyed("$defs", &object.defs, walk);
    listed("allOf", object.all_of.as_deref(), walk);
    listed("anyOf", object.any_of.as_deref(), walk);
    listed("oneOf", object.one_of.as_deref(), walk);
    single("not", object.not.as_deref(), walk);
    single("if", object.if_schema.as_deref(), walk);
    single("then", object.then_schema.as_deref(), walk);
    single("else", object.else_schema.as_deref(), walk);
    keyed("dependentSchemas", &object.dependent_schemas, walk);
    listed("prefixItems", object.prefix_items.as_deref(), walk);
    single("items", object.items.as_deref(), walk);
    single("contains", object.contains.as_deref(), walk);
    keyed("properties", &object.properties, walk);
    keyed("patternProperties", &object.pattern_properties, walk);
    single(
        "additionalProperties",
        object.additional_properties.as_deref(),
        walk,
    );
    single("propertyNames", object.property_names.as_deref(), walk);
    single(
        "unevaluatedItems",
        object.unevaluated_items.as_deref(),
        walk,
    );
    single(
        "unevaluatedProperties",
        object.unevaluated_properties.as_deref(),
        walk,
    );
    single("contentSchema", object.content_schema.as_deref(), walk);
}
