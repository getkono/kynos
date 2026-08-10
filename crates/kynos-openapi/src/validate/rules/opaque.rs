//! Opacity rules: what an `unchecked` waiver must leave behind, and what a
//! consumer is entitled to be told about it.
//!
//! Nothing here decides whether a waiver was wise. The rules check only that
//! the record of one is present, readable, and summarized at the root — so
//! that a description is never quietly less complete than it looks.

use crate::{
    annotation::{
        NOT_AUTHORITATIVE_ANNOTATION, OPAQUE_OPERATION_ANNOTATION, OPAQUE_ROUTES_ANNOTATION,
        Opaque, OpaqueRoute, is_authoritative,
    },
    model::{document::Document, paths::item::PathItem, paths::operation::Operation},
    validate::violation::{SpecError, Violation},
};

pub(in crate::validate) fn check_opaque(document: &Document, violations: &mut Vec<Violation>) {
    for (raw, item) in &document.paths.0 {
        check_item(&format!("#/paths/{raw}"), item, violations);
    }
    for (name, item) in &document.webhooks {
        check_item(&format!("#/webhooks/{name}"), item, violations);
    }
    for (name, item) in &document.components.path_items {
        check_item(&format!("#/components/pathItems/{name}"), item, violations);
    }

    check_routes(document, violations);

    if !is_authoritative(document) {
        violations.push(Violation::warning("#", SpecError::NotAuthoritative));

        if document.extensions.get(NOT_AUTHORITATIVE_ANNOTATION) != Some(&true.into()) {
            violations.push(Violation::error("#", SpecError::AuthorityNotStamped));
        }
    }
}

fn check_item(location: &str, item: &PathItem, violations: &mut Vec<Violation>) {
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
            &format!("{location}/additionalOperations/{method}"),
            operation,
            violations,
        );
    }
}

fn check_operation(location: &str, operation: &Operation, violations: &mut Vec<Violation>) {
    if !Opaque::is_annotated(operation) {
        return;
    }

    let Some(marker) = Opaque::of(operation) else {
        violations.push(Violation::error(
            location,
            SpecError::MalformedAnnotation {
                name: OPAQUE_OPERATION_ANNOTATION.to_owned(),
            },
        ));
        return;
    };

    violations.push(Violation::warning(
        location,
        SpecError::OpaqueOperation {
            reasons: marker.reasons,
        },
    ));
}

fn check_routes(document: &Document, violations: &mut Vec<Violation>) {
    let Some(routes) = OpaqueRoute::all(document) else {
        violations.push(Violation::error(
            "#",
            SpecError::MalformedAnnotation {
                name: OPAQUE_ROUTES_ANNOTATION.to_owned(),
            },
        ));
        return;
    };

    for (index, route) in routes.iter().enumerate() {
        violations.push(Violation::warning(
            format!("#/{OPAQUE_ROUTES_ANNOTATION}/{index}"),
            SpecError::OpaqueRoute {
                pattern: route.pattern.clone(),
            },
        ));
    }
}
