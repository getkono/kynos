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
        components::Components,
        document::{Document, SpecVersion},
        parameter::{
            Parameter, ParameterShape,
            header::{Header, HeaderShape},
        },
        paths::{item::PathItem, operation::Operation},
        reference::RefOr,
        response::Response,
        schema::Schema,
    },
    pointer::At,
    validate::{
        rules::{content::for_each_encoding, discriminator::Discriminators},
        violation::{SpecError, Violation},
    },
    visit::{self, Visit},
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
    visit::routes(document, walk);

    let components = &document.components;
    let at = At::ROOT.key("components");
    let schemas = at.key("schemas");
    for (name, schema) in &components.schemas {
        check_schema(schemas.key(name), schema, walk);
    }
    visit::inline_items(
        at.key("responses"),
        &components.responses,
        |at, response| {
            check_response(at, response, walk);
        },
    );
    visit::inline_items(
        at.key("parameters"),
        &components.parameters,
        |at, parameter| check_parameter(at, parameter, walk),
    );
    visit::inline_items(
        at.key("requestBodies"),
        &components.request_bodies,
        |at, body| check_request_body(at, body, walk),
    );
    visit::inline_items(at.key("headers"), &components.headers, |at, header| {
        check_header(at, header, walk);
    });
    let callbacks = at.key("callbacks");
    for (name, callback) in &components.callbacks {
        if let Some(callback) = callback.as_item() {
            visit::callback(callbacks.key(name), callback, walk);
        }
    }
    let path_items = at.key("pathItems");
    for (name, item) in &components.path_items {
        visit::path_item(path_items.key(name), item, walk);
    }
    #[cfg(feature = "openapi32")]
    visit::inline_items(
        at.key("mediaTypes"),
        &components.media_types,
        |at, media_type| check_media_type(at, media_type, walk),
    );
}

/// The schemas each Path Item and operation holds; the walk reaches their
/// callbacks' after them.
impl<'doc> Visit<'doc> for Walk<'_> {
    fn path_item(&mut self, at: At<'_>, item: &'doc PathItem, _path: Option<&'doc str>) -> bool {
        check_parameters(at, &item.parameters, self);
        true
    }

    fn operation(&mut self, at: At<'_>, _item: &'doc PathItem, operation: &'doc Operation) {
        check_parameters(at, &operation.parameters, self);

        if let Some(RefOr::Item(body)) = &operation.request_body {
            check_request_body(at.key("requestBody"), body, self);
        }

        let responses = at.key("responses");
        for (key, response) in operation.responses.entries() {
            check_response(responses.key(key), response, self);
        }
    }
}

fn check_parameters(at: At<'_>, parameters: &[RefOr<Parameter>], walk: &mut Walk<'_>) {
    let at = at.key("parameters");
    for (index, parameter) in parameters.iter().enumerate() {
        if let RefOr::Item(parameter) = parameter {
            check_parameter(at.index(index), parameter, walk);
        }
    }
}

fn check_parameter(at: At<'_>, parameter: &Parameter, walk: &mut Walk<'_>) {
    match parameter.shape() {
        ParameterShape::Schema { schema, .. } => check_schema(at.key("schema"), schema, walk),
        ParameterShape::Content { media_type, value } => {
            if let RefOr::Item(value) = &**value {
                check_media_type(at.key("content").key(media_type), value, walk);
            }
        }
    }
}

fn check_header(at: At<'_>, header: &Header, walk: &mut Walk<'_>) {
    match header.shape() {
        HeaderShape::Schema { schema, .. } => check_schema(at.key("schema"), schema, walk),
        HeaderShape::Content { media_type, value } => {
            if let RefOr::Item(value) = &**value {
                check_media_type(at.key("content").key(media_type), value, walk);
            }
        }
    }
}

fn check_request_body(at: At<'_>, body: &RequestBody, walk: &mut Walk<'_>) {
    check_content(at, &body.content, walk);
}

fn check_response(at: At<'_>, response: &Response, walk: &mut Walk<'_>) {
    visit::inline_items(at.key("headers"), &response.headers, |at, header| {
        check_header(at, header, walk);
    });
    check_content(at, &response.content, walk);
}

fn check_content(at: At<'_>, content: &Map<RefOr<MediaType>>, walk: &mut Walk<'_>) {
    visit::inline_items(at.key("content"), content, |at, media_type| {
        check_media_type(at, media_type, walk);
    });
}

fn check_media_type(at: At<'_>, media_type: &MediaType, walk: &mut Walk<'_>) {
    if let Some(schema) = &media_type.schema {
        check_payload(at.key("schema"), schema, walk);
    }
    #[cfg(feature = "openapi32")]
    if let Some(schema) = &media_type.item_schema {
        check_payload(at.key("itemSchema"), schema, walk);
    }

    // The headers of every encoded part, at every depth.
    for_each_encoding(at, media_type, &mut |at, encoding| {
        visit::inline_items(at.key("headers"), &encoding.headers, |at, header| {
            check_header(at, header, walk);
        });
    });
}

/// A media type's own schema, the one position where `true` is the payload.
///
/// Below it `true` is an ordinary keyword value, such as the problem
/// document's `additionalProperties: true`.
fn check_payload(at: At<'_>, schema: &Schema, walk: &mut Walk<'_>) {
    if matches!(schema, Schema::Bool(true)) {
        walk.violations
            .push(Violation::warning(at.pointer(), SpecError::UncheckedSchema));
    }
    check_schema(at, schema, walk);
}

/// One schema's own rules, then every subschema below it.
fn check_schema(at: At<'_>, schema: &Schema, walk: &mut Walk<'_>) {
    let Schema::Object(object) = schema else {
        return;
    };
    let location = at.pointer();
    if object
        .unknown_keywords
        .contains_key(UNCHECKED_SCHEMA_ANNOTATION)
    {
        walk.violations
            .push(Violation::warning(&location, SpecError::UncheckedSchema));
    }
    walk.discriminators.visit(
        &location,
        object,
        walk.components,
        walk.version,
        &mut walk.violations,
    );
    object.for_each_subschema(at, |at, subschema| check_schema(at, subschema, walk));
}
