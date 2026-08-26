//! Structural validation of a [`Document`].
//!
//! Everything checked here is a rule the OpenAPI specification states but that
//! the type system cannot enforce on its own — uniqueness across a whole
//! document, correspondence between a path template and its parameters, names
//! that must resolve against what the document declares elsewhere.
//!
//! Mutual exclusions between fields are deliberately not among them. The model
//! spells those as types, so a document that violates one can be neither built
//! nor parsed and never reaches a rule here.
//!
//! Kynos runs this when a router is built, so a description that would mislead
//! a client generator fails at startup rather than being published.
//!
//! [`Validator::validate`] is the orchestrator; the rules themselves are
//! internal, one module per family of specification rule.

pub mod violation;

// The rules are implementation, not surface: a caller consumes
// [`Violation`]s, never a checking function.
mod rules;

use crate::{
    model::document::{Document, SpecVersion},
    validate::{
        rules::{extensions::check_extensions, opaque::check_opaque},
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

        // OpenAPI 3.2 requires a document to declare something; 3.1 does not.
        if self.version.supports_3_2()
            && document.paths.is_empty()
            && document.webhooks.is_empty()
            && document.components.is_empty()
        {
            violations.push(Violation::error("#", SpecError::EmptyDocument));
        }

        // A License Object setting both `identifier` and `url` used to be
        // checked here. `License` now holds at most one of the two, so a
        // document carrying both cannot reach this function: it fails to
        // deserialize, and there is no way to build one.

        self.check_servers(document, &mut violations);
        self.check_tags(document, &mut violations);
        self.check_component_names(document, &mut violations);
        self.check_paths(document, &mut violations);
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
