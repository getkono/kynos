//! Media type rules, and the request bodies and responses that hold them.
//!
//! A media type's schema is read here only to judge its encodings against
//! it, through `allOf` and `#/components/schemas` references: the properties
//! it declares and whether it can be an array. `rules/schemas.rs` checks the
//! schemas themselves, every one at once.

use std::collections::HashSet;

#[cfg(feature = "openapi32")]
use crate::{
    Map,
    model::schema::types::{SchemaType, TypeSet},
};
use crate::{
    model::{
        body::{RequestBody, encoding::Encoding, media_type::MediaType},
        components::Components,
        document::Document,
        response::Response,
        schema::{Schema, object::SchemaObject},
    },
    validate::{
        rules::{document::check_link, parameters::check_header_map},
        violation::{SpecError, Violation, pointer_token},
    },
};

/// Checks each reusable response, request body and media type once, where it
/// is defined.
///
/// Not once per reference; one that is itself a `$ref` is checked where it
/// leads.
pub(in crate::validate) fn check_component_content(
    document: &Document,
    violations: &mut Vec<Violation>,
) {
    let components = &document.components;
    for (name, response) in &components.responses {
        if let Some(response) = response.as_item() {
            check_response(
                &format!("#/components/responses/{}", pointer_token(name)),
                response,
                components,
                violations,
            );
        }
    }
    for (name, body) in &components.request_bodies {
        if let Some(body) = body.as_item() {
            check_request_body(
                &format!("#/components/requestBodies/{}", pointer_token(name)),
                body,
                components,
                violations,
            );
        }
    }
    #[cfg(feature = "openapi32")]
    for (name, media_type) in &components.media_types {
        if let Some(media_type) = media_type.as_item() {
            check_media_type(
                &format!("#/components/mediaTypes/{}", pointer_token(name)),
                media_type,
                components,
                violations,
            );
        }
    }
}

pub(in crate::validate) fn check_request_body(
    location: &str,
    body: &RequestBody,
    components: &Components,
    violations: &mut Vec<Violation>,
) {
    for (media_type, content) in &body.content {
        check_media_type(
            &format!("{location}/content/{}", pointer_token(media_type)),
            content,
            components,
            violations,
        );
    }
}

pub(in crate::validate) fn check_response(
    location: &str,
    response: &Response,
    components: &Components,
    violations: &mut Vec<Violation>,
) {
    for (media_type, content) in &response.content {
        check_media_type(
            &format!("{location}/content/{}", pointer_token(media_type)),
            content,
            components,
            violations,
        );
    }
    // An inline link's server; a referenced one is checked at its component.
    for (name, link) in &response.links {
        if let Some(link) = link.as_item() {
            check_link(
                &format!("{location}/links/{}", pointer_token(name)),
                link,
                violations,
            );
        }
    }
    // A link's target and a header's shape hold by construction; a header's
    // name is a map key no type reaches, so it is still checked.
    check_header_map(
        &format!("{location}/headers"),
        &response.headers,
        violations,
    );
}

/// Checks a media type's encodings against its schema, then each encoding.
///
/// The schema-dependent rules hold at this level only: a nested encoding
/// applies to a part whose value the specification leaves to the part's own
/// media type, so there is no schema to hold its keys against.
pub(in crate::validate) fn check_media_type(
    location: &str,
    content: &MediaType,
    components: &Components,
    violations: &mut Vec<Violation>,
) {
    #[cfg(feature = "openapi32")]
    {
        check_exclusion(
            location,
            &content.encoding,
            content.prefix_encoding.as_ref(),
            content.item_encoding.as_deref(),
            violations,
        );
        // `references/3.2.0.md:1386`.
        if (content.prefix_encoding.is_some() || content.item_encoding.is_some())
            && content.item_schema.is_none()
            && content
                .schema
                .as_ref()
                .is_none_or(|schema| excludes_arrays(schema, components))
        {
            violations.push(Violation::error(
                location,
                SpecError::PositionalEncodingWithoutArray,
            ));
        }
    }

    // `references/3.1.2.md:1551`, `references/3.2.0.md:1368`.
    let properties = match &content.schema {
        Some(schema) => declared_properties(schema, components),
        None => Some(HashSet::new()),
    };
    for (property, encoding) in &content.encoding {
        let location = format!("{location}/encoding/{}", pointer_token(property));
        if properties
            .as_ref()
            .is_some_and(|properties| !properties.contains(property.as_str()))
        {
            violations.push(Violation::error(
                &location,
                SpecError::UnknownEncodingProperty {
                    property: property.clone(),
                },
            ));
        }
        check_encoding(&location, encoding, violations);
    }
    #[cfg(feature = "openapi32")]
    {
        for (index, encoding) in content.prefix_encoding.iter().flatten().enumerate() {
            check_encoding(
                &format!("{location}/prefixEncoding/{index}"),
                encoding,
                violations,
            );
        }
        if let Some(encoding) = &content.item_encoding {
            check_encoding(&format!("{location}/itemEncoding"), encoding, violations);
        }
    }
}

/// An encoded part's headers, and under 3.2 the parts nested inside it, which
/// apply their fields "in the same manner" as a media type
/// (`references/3.2.0.md:1711-1713`).
fn check_encoding(location: &str, encoding: &Encoding, violations: &mut Vec<Violation>) {
    #[cfg(feature = "openapi32")]
    check_exclusion(
        location,
        &encoding.encoding,
        encoding.prefix_encoding.as_ref(),
        encoding.item_encoding.as_deref(),
        violations,
    );
    check_header_map(
        &format!("{location}/headers"),
        &encoding.headers,
        violations,
    );
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

/// The one stated exclusion not spelled as a type: `prefixEncoding` and
/// `itemEncoding` are 3.2-only, so a sum type would change shape with the
/// `openapi32` feature, breaking additivity.
#[cfg(feature = "openapi32")]
fn check_exclusion(
    location: &str,
    encoding: &Map<Encoding>,
    prefix_encoding: Option<&Vec<Encoding>>,
    item_encoding: Option<&Encoding>,
    violations: &mut Vec<Violation>,
) {
    if !encoding.is_empty() && (prefix_encoding.is_some() || item_encoding.is_some()) {
        violations.push(Violation::error(location, SpecError::ConflictingEncoding));
    }
}

/// The property names `schema` declares, or `None` where a keyword this rule
/// does not read could declare more.
///
/// Read are `properties` on `schema` and on every schema it is the
/// conjunction of. `anyOf`, `oneOf`, a conditional, `dependentSchemas` and
/// `$dynamicRef` can each declare one unseen, as can a conjunct no object
/// stands for.
fn declared_properties<'doc>(
    schema: &'doc Schema,
    components: &'doc Components,
) -> Option<HashSet<&'doc str>> {
    let mut properties = HashSet::new();
    let mut open = false;
    let complete = for_each_conjunct(schema, components, |object| {
        properties.extend(object.properties.keys().map(String::as_str));
        open |= object.any_of.is_some()
            || object.one_of.is_some()
            || object.if_schema.is_some()
            || object.then_schema.is_some()
            || object.else_schema.is_some()
            || !object.dependent_schemas.is_empty()
            || object.dynamic_ref.is_some();
    });
    (complete && !open).then_some(properties)
}

/// Whether no instance of `schema` can be an array: some conjunct's `type`
/// omits `array`.
#[cfg(feature = "openapi32")]
fn excludes_arrays(schema: &Schema, components: &Components) -> bool {
    let mut excludes = false;
    for_each_conjunct(schema, components, |object| {
        excludes |= match &object.ty {
            Some(TypeSet::One(ty)) => *ty != SchemaType::Array,
            Some(TypeSet::Many(types)) => !types.contains(&SchemaType::Array),
            None => false,
        };
    });
    excludes
}

/// Visits `schema` and each schema it is the conjunction of: its `allOf`
/// members and the `#/components/schemas` entry its `$ref` names, each
/// component once however many paths reach it.
///
/// Returns whether every conjunct was an object this document resolves; a
/// boolean schema or a reference it cannot follow is not visited. A name is
/// looked up as written: a valid component name holds no character a pointer
/// escapes, so an escaped or deeper pointer misses and counts as unfollowed.
fn for_each_conjunct<'doc>(
    schema: &'doc Schema,
    components: &'doc Components,
    mut visit: impl FnMut(&'doc SchemaObject),
) -> bool {
    let mut pending = vec![schema];
    let mut followed = HashSet::new();
    let mut complete = true;
    while let Some(schema) = pending.pop() {
        let Schema::Object(object) = schema else {
            complete = false;
            continue;
        };
        visit(object);
        pending.extend(object.all_of.iter().flatten());
        if let Some(reference) = &object.reference {
            match reference
                .strip_prefix("#/components/schemas/")
                .and_then(|name| components.schemas.get_key_value(name))
            {
                Some((name, target)) => {
                    if followed.insert(name.as_str()) {
                        pending.push(target);
                    }
                }
                None => complete = false,
            }
        }
    }
    complete
}
