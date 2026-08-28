//! Rules that apply to the document as a whole: servers, tags, component
//! names.

use std::collections::HashSet;

#[cfg(feature = "openapi32")]
use std::collections::HashMap;

use crate::{
    model::{components::ComponentName, document::Document},
    validate::violation::{SpecError, Violation},
};

pub(in crate::validate) fn check_servers(document: &Document, violations: &mut Vec<Violation>) {
    for (index, server) in document.servers.iter().enumerate() {
        for (name, variable) in &server.variables {
            let location = format!("#/servers/{index}/variables/{name}");
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

        // Walk upward, bounded by the number of tags: a chain longer than
        // that has necessarily revisited a node.
        let mut current = parent;
        let mut steps = 0;
        while let Some(next) = parents.get(current) {
            if *next == tag.name.as_str() || steps > document.tags.len() {
                violations.push(Violation::error(
                    &location,
                    SpecError::TagParentCycle {
                        name: tag.name.clone(),
                    },
                ));
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

    // Every section, because the specification says "**All** the fixed
    // fields declared above". Five of them were consulted, which let a key
    // in any of the others through whatever it was. `extensions` is not a
    // section: its keys are `x-` prefixed names, checked by their own rule.
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
                    format!("#/components/{group}/{name}"),
                    SpecError::InvalidComponentName { name: name.clone() },
                ));
            }
        }
    }
}
