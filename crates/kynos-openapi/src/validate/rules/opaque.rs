//! Opacity rules: what an `unchecked` waiver must leave behind, and what a
//! consumer is entitled to be told about it.
//!
//! The rules check only that a waiver's record is present, readable, and
//! summarized at the root. The callback walk they need also carries the rule
//! on callback keys.

use crate::{
    annotation::{
        MalformedAnnotation, NOT_AUTHORITATIVE_ANNOTATION, OPAQUE_ROUTES_ANNOTATION, Opaque,
        OpaqueRoute,
    },
    model::{
        callback::Callback,
        document::Document,
        extensions::EXTENSION_PREFIX,
        paths::{item::PathItem, operation::Operation},
        reference::RefOr,
    },
    validate::violation::{SpecError, Violation, pointer_token},
};

pub(in crate::validate) fn check_opaque(document: &Document, violations: &mut Vec<Violation>) {
    for (raw, item) in &document.paths.items {
        check_item(&format!("#/paths/{}", pointer_token(raw)), item, violations);
    }
    for (name, item) in &document.webhooks {
        check_item(
            &format!("#/webhooks/{}", pointer_token(name)),
            item,
            violations,
        );
    }
    for (name, item) in &document.components.path_items {
        check_item(
            &format!("#/components/pathItems/{}", pointer_token(name)),
            item,
            violations,
        );
    }
    for (name, callback) in &document.components.callbacks {
        check_callback(
            &format!("#/components/callbacks/{}", pointer_token(name)),
            callback,
            violations,
        );
    }

    check_routes(document, violations);

    if !document.is_authoritative() {
        if document.extensions.get(NOT_AUTHORITATIVE_ANNOTATION) == Some(&true.into()) {
            // Honestly incomplete: worth saying, not worth failing over.
            violations.push(Violation::warning("#", SpecError::NotAuthoritative));
        } else {
            // A missing root stamp is the graver claim; reporting both would
            // bury it.
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
            &format!("{location}/additionalOperations/{}", pointer_token(method)),
            operation,
            violations,
        );
    }
}

/// A callback holds path items of its own, and an operation inside one is as
/// much part of the service as any other.
///
/// This walk reaches every callback that is not a `$ref` exactly once, so it
/// also carries the one rule about a callback's keys: an item keyed `x-` would
/// be read back as an extension.
fn check_callback(location: &str, callback: &RefOr<Callback>, violations: &mut Vec<Violation>) {
    let Some(callback) = callback.as_item() else {
        return;
    };
    for (expression, item) in &callback.items {
        let location = format!("{location}/{}", pointer_token(expression));
        if expression.starts_with(EXTENSION_PREFIX) {
            violations.push(Violation::error(
                &location,
                SpecError::CallbackKeyedAsExtension {
                    expression: expression.clone(),
                },
            ));
        }
        if let Some(item) = item.as_item() {
            check_item(&location, item, violations);
        }
    }
}

fn check_operation(location: &str, operation: &Operation, violations: &mut Vec<Violation>) {
    for (name, callback) in &operation.callbacks {
        check_callback(
            &format!("{location}/callbacks/{}", pointer_token(name)),
            callback,
            violations,
        );
    }

    if !Opaque::is_annotated(operation) {
        return;
    }

    match Opaque::of(operation) {
        Ok(Some(marker)) => violations.push(Violation::warning(
            location,
            SpecError::OpaqueOperation {
                reasons: marker.reasons,
            },
        )),
        // `is_annotated` was true, so the annotation is present.
        Ok(None) => {}
        Err(error) => violations.push(malformed(location, &error)),
    }
}

fn check_routes(document: &Document, violations: &mut Vec<Violation>) {
    let routes = match OpaqueRoute::all(document) {
        Ok(routes) => routes,
        Err(error) => {
            violations.push(malformed("#", &error));
            return;
        }
    };

    for (index, route) in routes.iter().enumerate() {
        violations.push(Violation::warning(
            format!("#/{}/{index}", pointer_token(OPAQUE_ROUTES_ANNOTATION)),
            SpecError::OpaqueRoute {
                pattern: route.pattern.clone(),
            },
        ));
    }
}

fn malformed(location: impl Into<String>, error: &MalformedAnnotation) -> Violation {
    Violation::error(
        location,
        SpecError::MalformedAnnotation {
            name: error.name.clone(),
            detail: error.detail.clone(),
        },
    )
}
