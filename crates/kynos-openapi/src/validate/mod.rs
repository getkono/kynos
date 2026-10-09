//! Structural validation of a [`Document`].
//!
//! Everything checked here is a rule the OpenAPI specification states but that
//! the type system cannot enforce — uniqueness across a whole document,
//! correspondence between a path template and its parameters, names that must
//! resolve. Mutual exclusions between fields are spelled as types instead.
//!
//! Kynos runs this when a router is built, so a misleading description fails
//! at startup rather than being published.

pub mod violation;

// Internal: a caller consumes [`Violation`]s, never a checking function.
mod rules;

use crate::{
    model::document::{Document, SpecVersion},
    validate::{
        rules::{
            document::{
                check_component_names, check_component_parameters, check_servers, check_tags,
            },
            extensions::check_extensions,
            opaque::check_opaque,
            schemas::check_unchecked_schemas,
        },
        violation::{Severity, SpecError, Violation},
    },
};

/// Checks a document against the rules of a specification version.
#[derive(Clone, Copy, Debug)]
pub struct Validator {
    version: SpecVersion,
}

impl Validator {
    /// Creates a validator for `version`.
    #[must_use]
    pub fn new(version: SpecVersion) -> Self {
        Self { version }
    }

    /// Collects every violation in `document`, most structural first.
    #[must_use]
    pub fn validate(&self, document: &Document) -> Vec<Violation> {
        let mut violations = Vec::new();

        // Validating as 3.1 refuses 3.2 constructs by the same walk
        // `Document::emit` refuses on, so validating and emitting agree.
        if !self.version.supports_3_2() {
            let blockers = crate::emit::downgrade::three_two_only_constructs(document);
            if !blockers.is_empty() {
                violations.push(Violation::error("#", SpecError::RequiresV3_2 { blockers }));
            }
        }

        check_servers(document, &mut violations);
        check_tags(document, &mut violations);
        check_component_names(document, &mut violations);
        check_component_parameters(document, &mut violations);
        self.check_security(document, &mut violations);
        self.check_paths(document, &mut violations);
        check_unchecked_schemas(document, &mut violations);
        check_opaque(document, &mut violations);
        check_extensions("#", &document.extensions, &mut violations);

        violations
    }
}

impl Document {
    /// Validates this document against the rules of `version`.
    ///
    /// # Errors
    ///
    /// Returns every [`Severity::Error`] violation found. Warnings are
    /// discarded; use [`Validator::validate`] to see them.
    pub fn validate(&self, version: SpecVersion) -> Result<(), Vec<Violation>> {
        let errors: Vec<Violation> = Validator::new(version)
            .validate(self)
            .into_iter()
            .filter(|violation| violation.severity == Severity::Error)
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests;
