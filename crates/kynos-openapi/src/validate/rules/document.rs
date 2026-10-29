//! Rules that apply to the document as a whole: servers, tags, component
//! names, reusable parameters. The Server Object rule is shared with every
//! other scope a server is declared at.

use std::collections::HashSet;

#[cfg(feature = "openapi32")]
use std::collections::HashMap;

use crate::{
    model::{components::ComponentName, document::Document, link::Link, server::Server},
    pointer::{At, pointer_token},
    validate::{
        rules::parameters::check_parameter,
        violation::{SpecError, Violation},
    },
    visit,
};

/// Checks the root `servers`, and the server of each reusable link once, where
/// it is defined.
///
/// A Path Item's, an Operation's and an inline Link's servers are checked by
/// the walk that reaches them.
pub(in crate::validate) fn check_servers(document: &Document, violations: &mut Vec<Violation>) {
    check_server_list("#", &document.servers, violations);
    visit::inline_items(
        At::ROOT.key("components").key("links"),
        &document.components.links,
        |at, link| check_link(&at.pointer(), link, violations),
    );
}

/// Checks each Server Object in the `servers` field of the object at
/// `location`.
pub(in crate::validate) fn check_server_list(
    location: &str,
    servers: &[Server],
    violations: &mut Vec<Violation>,
) {
    for (index, server) in servers.iter().enumerate() {
        check_server(&format!("{location}/servers/{index}"), server, violations);
    }
}

/// Checks the server of the Link Object at `location`.
pub(in crate::validate) fn check_link(
    location: &str,
    link: &Link,
    violations: &mut Vec<Violation>,
) {
    if let Some(server) = &link.server {
        check_server(&format!("{location}/server"), server, violations);
    }
}

/// Checks one Server Object's variables: an `enum` MUST NOT be empty, and the
/// `default` MUST be one of its values.
fn check_server(location: &str, server: &Server, violations: &mut Vec<Violation>) {
    for (name, variable) in &server.variables {
        let location = format!("{location}/variables/{}", pointer_token(name));
        if let Some(values) = &variable.enumeration {
            if values.is_empty() {
                violations.push(Violation::error(
                    &location,
                    SpecError::EmptyServerVariableEnum { name: name.clone() },
                ));
            } else if !values.contains(&variable.default_value) {
                violations.push(Violation::error(
                    &location,
                    SpecError::ServerVariableDefaultNotInEnum { name: name.clone() },
                ));
            }
        }
    }
}

pub(in crate::validate) fn check_tags(document: &Document, violations: &mut Vec<Violation>) {
    let mut seen: HashSet<&str> = HashSet::new();
    for (index, tag) in document.tags.iter().enumerate() {
        if !seen.insert(tag.name.as_str()) {
            violations.push(Violation::error(
                format!("#/tags/{index}"),
                SpecError::DuplicateTag {
                    name: tag.name.clone(),
                },
            ));
        }
    }

    #[cfg(feature = "openapi32")]
    check_tag_hierarchy(document, &seen, violations);

    #[cfg(not(feature = "openapi32"))]
    let _ = &seen;
}

#[cfg(feature = "openapi32")]
pub(in crate::validate) fn check_tag_hierarchy(
    document: &Document,
    declared: &HashSet<&str>,
    violations: &mut Vec<Violation>,
) {
    let parents: HashMap<&str, &str> = document
        .tags
        .iter()
        .filter_map(|tag| {
            tag.parent
                .as_deref()
                .map(|parent| (tag.name.as_str(), parent))
        })
        .collect();

    for (index, tag) in document.tags.iter().enumerate() {
        let Some(parent) = tag.parent.as_deref() else {
            continue;
        };
        let location = format!("#/tags/{index}");

        if !declared.contains(parent) {
            violations.push(Violation::error(
                &location,
                SpecError::UnknownTagParent {
                    name: tag.name.clone(),
                    parent: parent.to_owned(),
                },
            ));
            continue;
        }

        // Walk upward until the chain returns to this tag, which puts it on
        // a cycle. The walk is bounded by the number of tags: a chain longer
        // than that has entered a cycle this tag is not on, which is reported
        // at that cycle's own members rather than here.
        let mut current = parent;
        let mut steps = 0;
        while let Some(next) = parents.get(current) {
            if *next == tag.name.as_str() {
                violations.push(Violation::error(
                    &location,
                    SpecError::TagParentCycle {
                        name: tag.name.clone(),
                    },
                ));
                break;
            }
            if steps > document.tags.len() {
                break;
            }
            current = next;
            steps += 1;
        }
    }
}

pub(in crate::validate) fn check_component_names(
    document: &Document,
    violations: &mut Vec<Violation>,
) {
    let components = &document.components;

    // Every section: the specification says "**All** the fixed fields declared
    // above". `extensions` keys are checked by their own rule.
    let groups = [
        ("schemas", components.schemas.keys().collect::<Vec<_>>()),
        ("responses", components.responses.keys().collect()),
        ("parameters", components.parameters.keys().collect()),
        ("examples", components.examples.keys().collect()),
        ("requestBodies", components.request_bodies.keys().collect()),
        ("headers", components.headers.keys().collect()),
        (
            "securitySchemes",
            components.security_schemes.keys().collect(),
        ),
        ("links", components.links.keys().collect()),
        ("callbacks", components.callbacks.keys().collect()),
        ("pathItems", components.path_items.keys().collect()),
        #[cfg(feature = "openapi32")]
        ("mediaTypes", components.media_types.keys().collect()),
    ];

    for (group, names) in groups {
        for name in names {
            if !ComponentName::is_valid(name) {
                violations.push(Violation::error(
                    format!("#/components/{group}/{}", pointer_token(name)),
                    SpecError::InvalidComponentName { name: name.clone() },
                ));
            }
        }
    }
}

/// Checks each reusable parameter once, where it is defined.
///
/// Not once per reference; one that is itself a `$ref` is checked where it
/// leads.
pub(in crate::validate) fn check_component_parameters(
    document: &Document,
    violations: &mut Vec<Violation>,
) {
    visit::inline_items(
        At::ROOT.key("components").key("parameters"),
        &document.components.parameters,
        |at, parameter| check_parameter(&at.pointer(), parameter, violations),
    );
}
