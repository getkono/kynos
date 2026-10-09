//! What stands in the way of emitting a document as an earlier version.

mod unrecognised;

#[cfg(feature = "openapi32")]
mod walk;

#[cfg(feature = "openapi32")]
use walk::{collect_components_blockers, collect_path_item_blockers, collect_servers_blockers};

use crate::model::document::Document;
#[cfg(feature = "openapi32")]
use crate::validate::violation::pointer_token;

// Reached only while collecting 3.2 blockers.
#[cfg(feature = "openapi32")]
use crate::model::{
    body::{encoding::Encoding, media_type::MediaType},
    callback::Callback,
    components::Components,
    example::{Example, ExampleValue, Examples},
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
/// Each entry is a location. Two kinds are listed:
///
/// - each 3.2-only field the model types, which only a build with the
///   `openapi32` feature can hold;
/// - in every build, each field the model does not recognise, kept in an
///   object's [`Extensions`](crate::model::extensions::Extensions) because its
///   name lacks the `x-` prefix (where a build without `openapi32` keeps every
///   3.2 field).
#[must_use]
pub fn three_two_only_constructs(document: &Document) -> Vec<String> {
    let mut blockers = Vec::new();
    #[cfg(feature = "openapi32")]
    collect_three_two_fields(document, &mut blockers);
    unrecognised::collect_unrecognised_fields(document, &mut blockers);
    blockers
}

/// Lists the fields in a document that the model does not recognise.
///
/// Each is a key without the `x-` prefix kept in an object's
/// [`Extensions`](crate::model::extensions::Extensions), at the location it was
/// written: the blockers [`three_two_only_constructs`] lists in every build.
/// With `openapi32`, each is a field neither version holds, such as a
/// misspelling or an extension missing its prefix.
#[must_use]
pub fn unrecognised_fields(document: &Document) -> Vec<String> {
    let mut found = Vec::new();
    unrecognised::collect_unrecognised_fields(document, &mut found);
    found
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
    collect_components_blockers("#/components", &document.components, blockers);

    for (raw, item) in &document.paths.items {
        collect_path_item_blockers(&format!("#/paths/{}", pointer_token(raw)), item, blockers);
    }

    // A webhook is a Path Item, and every 3.2 construct one can carry is
    // one a Path Item under `paths` can carry.
    for (name, item) in &document.webhooks {
        collect_path_item_blockers(
            &format!("#/webhooks/{}", pointer_token(name)),
            item,
            blockers,
        );
    }
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
