//! What stands in the way of emitting a document as an earlier version.

pub(crate) mod unrecognised;

#[cfg(feature = "openapi32")]
mod walk;

#[cfg(feature = "openapi32")]
use walk::{Blockers, collect_components_blockers, collect_servers_blockers};

use crate::model::document::Document;
#[cfg(feature = "openapi32")]
use crate::pointer::pointer_token;

// Reached only while collecting 3.2 blockers.
#[cfg(feature = "openapi32")]
use crate::model::{
    body::{encoding::Encoding, media_type::MediaType},
    components::Components,
    example::{Example, ExampleValue},
    link::Link,
    parameter::{Parameter, ParameterIn, header::Header},
    paths::{item::PathItem, operation::Operation},
    reference::RefOr,
    response::{Response, Responses},
    schema::Schema,
    security::{SecurityScheme, oauth::OAuthFlows},
    server::Server,
};

/// Lists the constructs in a document that OpenAPI 3.1 cannot express.
///
/// Each entry is a location. Five kinds are listed, in this order:
///
/// - each 3.2-only field the model types, which only a build with the
///   `openapi32` feature can hold;
/// - in every build, each field the model does not recognise, kept in an
///   object's [`Extensions`](crate::model::extensions::Extensions) because its
///   name lacks the `x-` prefix (where a build without `openapi32` keeps every
///   3.2 field);
/// - in every build, each `content` entry given by reference, which only 3.2
///   admits, located at the entry;
/// - in every build, each inline response without a `description`, which 3.1
///   requires and 3.2 does not, located at the `description` it lacks;
/// - in every build, each security requirement name that names a scheme by
///   URI rather than by a declared component name, which only 3.2 admits,
///   located at the name.
#[must_use]
pub fn three_two_only_constructs(document: &Document) -> Vec<String> {
    let ThreeOneObstacles {
        mut fields,
        undescribed_responses,
        scheme_uris,
    } = three_one_obstacles(document);
    fields.extend(
        undescribed_responses
            .into_iter()
            .map(|response| response + "/description"),
    );
    fields.extend(scheme_uris);
    fields
}

/// What [`three_two_only_constructs`] lists, kept apart by kind so that
/// validation can report each the way its own rule names it.
pub(crate) struct ThreeOneObstacles {
    /// The 3.2 fields the model types, then the fields it does not recognise,
    /// then each `content` entry given by reference.
    pub(crate) fields: Vec<String>,
    /// Each inline response without a `description`, at the response itself.
    pub(crate) undescribed_responses: Vec<String>,
    /// Each security requirement name that is a scheme URI, at the name.
    pub(crate) scheme_uris: Vec<String>,
}

/// Collects what stands in the way of emitting `document` as 3.1.
pub(crate) fn three_one_obstacles(document: &Document) -> ThreeOneObstacles {
    let found = unrecognised::walk(document);
    let mut fields = Vec::new();
    #[cfg(feature = "openapi32")]
    collect_three_two_fields(document, &mut fields);
    fields.extend(found.fields.iter().map(unrecognised::Field::pointer));
    fields.extend(found.media_type_refs);
    ThreeOneObstacles {
        fields,
        undescribed_responses: found.undescribed_responses,
        scheme_uris: found.scheme_uris,
    }
}

/// Whether a security requirement name, if no scheme is declared under it,
/// reads as a Security Scheme Object's URI.
///
/// 3.2 admits a URI there (`references/3.2.0.md:4685`); 3.1 admits only a
/// declared component name (`references/3.1.2.md:4128`).
///
/// A *bare* single-segment name is not read as a URI: 3.2 spells a
/// single-segment relative reference `./foo`, so an undeclared `Bearer` is a
/// misspelling under either version, never something 3.2 expresses.
pub(crate) fn names_a_scheme_by_uri(name: &str) -> bool {
    name.contains('/') || name.contains(':') || name.contains('#')
}

/// Lists the fields in a document that the model does not recognise.
///
/// Each is a key without the `x-` prefix kept in an object's
/// [`Extensions`](crate::model::extensions::Extensions), at the location it was
/// written: the blockers [`three_two_only_constructs`] lists in every build
/// for a field, as opposed to an undescribed response or a requirement naming
/// a scheme by URI. With `openapi32`, each is a field neither version holds,
/// such as a
/// misspelling or an extension missing its prefix.
#[must_use]
pub fn unrecognised_fields(document: &Document) -> Vec<String> {
    unrecognised::walk(document)
        .fields
        .iter()
        .map(unrecognised::Field::pointer)
        .collect()
}

/// The 3.2-only fields the model types, which a build without `openapi32`
/// cannot hold.
#[cfg(feature = "openapi32")]
fn collect_three_two_fields(document: &Document, blockers: &mut Vec<String>) {
    if document.self_uri.is_some() {
        blockers.push("#/$self".to_owned());
    }
    collect_servers_blockers("#", &document.servers, blockers);
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
    collect_components_blockers(
        crate::pointer::At::ROOT.key("components"),
        &document.components,
        blockers,
    );

    // A webhook is a Path Item, and every 3.2 construct one can carry is
    // one a Path Item under `paths` can carry.
    crate::visit::routes(document, &mut Blockers { blockers });
}

/// Visits each present item of a `RefOr` map, at the pointer it lives at.
///
/// A `RefOr::Ref` is skipped: its target is walked where it is defined.
#[cfg(feature = "openapi32")]
fn for_each_item<'a, T: 'a>(
    section: &str,
    entries: impl IntoIterator<Item = (&'a String, &'a RefOr<T>)>,
    mut visit: impl FnMut(String, &T),
) {
    for (name, entry) in entries {
        if let RefOr::Item(item) = entry {
            visit(format!("{section}/{}", pointer_token(name)), item);
        }
    }
}

/// The same for a map holding its values directly.
#[cfg(feature = "openapi32")]
fn for_each<'a, T: 'a>(
    section: &str,
    entries: impl IntoIterator<Item = (&'a String, &'a T)>,
    mut visit: impl FnMut(String, &T),
) {
    for (name, item) in entries {
        visit(format!("{section}/{}", pointer_token(name)), item);
    }
}
