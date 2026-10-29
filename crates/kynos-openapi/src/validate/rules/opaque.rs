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
    model::{document::Document, extensions::EXTENSION_PREFIX, paths::operation::Operation},
    pointer::{At, pointer_token},
    validate::violation::{SpecError, Violation},
    visit::{self, Visit},
};

pub(in crate::validate) fn check_opaque(document: &Document, violations: &mut Vec<Violation>) {
    visit::document(document, &mut Operations { violations });

    check_routes(document, violations);

    let stamped = document.extensions.get(NOT_AUTHORITATIVE_ANNOTATION) == Some(&true.into());
    match (document.is_authoritative(), stamped) {
        (true, false) => {}
        // Stale: it understates the description, so it is not worth failing
        // over either.
        (true, true) => violations.push(Violation::warning("#", SpecError::StaleAuthorityStamp)),
        // Honestly incomplete: worth saying, not worth failing over.
        (false, true) => violations.push(Violation::warning("#", SpecError::NotAuthoritative)),
        // A missing root stamp is the graver claim; reporting both would bury
        // it.
        (false, false) => violations.push(Violation::error("#", SpecError::AuthorityNotStamped)),
    }
}

/// Every operation, wherever it is declared: a callback's are as much part of
/// the service as any other.
struct Operations<'v> {
    violations: &'v mut Vec<Violation>,
}

impl<'doc> Visit<'doc> for Operations<'_> {
    /// The walk reaches every callback that is not a `$ref` exactly once, so it
    /// also carries the one rule about a callback's keys: an item keyed `x-`
    /// would be read back as an extension.
    fn callback_entry(&mut self, at: At<'_>, expression: &'doc str) {
        if expression.starts_with(EXTENSION_PREFIX) {
            self.violations.push(Violation::error(
                at.pointer(),
                SpecError::CallbackKeyedAsExtension {
                    expression: expression.to_owned(),
                },
            ));
        }
    }

    /// After its callbacks, so their operations are reported first.
    fn operation_walked(&mut self, at: At<'_>, operation: &'doc Operation) {
        match Opaque::of(operation) {
            Ok(Some(marker)) => self.violations.push(Violation::warning(
                at.pointer(),
                SpecError::OpaqueOperation {
                    reasons: marker.reasons,
                },
            )),
            Ok(None) => {}
            Err(error) => self.violations.push(malformed(at.pointer(), &error)),
        }
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
