//! What stands in the way of emitting a document as an earlier version.

mod unrecognised;

// The whole module is 3.2-only, so the gate sits here rather than on each of
// its eighteen walkers.
#[cfg(feature = "openapi32")]
mod walk;

#[cfg(feature = "openapi32")]
use walk::{collect_components_blockers, collect_path_item_blockers, collect_servers_blockers};

use crate::model::document::Document;
#[cfg(feature = "openapi32")]
use crate::validate::violation::pointer_token;

// Everything below the document itself is reached only while collecting 3.2
// blockers, which a build without `openapi32` cannot have any of.
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
/// Each entry is a location, suitable for telling the caller what stands in the
/// way of emitting the document as 3.1. Two kinds are listed:
///
/// - each 3.2-only field the model types, which only a build with the
///   `openapi32` feature can hold;
/// - in every build, each field the model does not recognise, kept in an
///   object's [`Extensions`](crate::model::extensions::Extensions) because its
///   name lacks the `x-` prefix. A build without `openapi32` parses every 3.2
///   field there, and any build parses a `$ref` written as a `content` entry
///   there, so neither is relabelled as 3.1.
#[must_use]
pub fn three_two_only_constructs(document: &Document) -> Vec<String> {
    let mut blockers = Vec::new();
    #[cfg(feature = "openapi32")]
    collect_three_two_fields(document, &mut blockers);
    unrecognised::collect_unrecognised_fields(document, &mut blockers);
    blockers
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
/// A `RefOr::Ref` is skipped rather than followed: it names an object defined
/// elsewhere in the document, and that definition is walked where it is
/// written. Following it here would report one construct once per reference.
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
