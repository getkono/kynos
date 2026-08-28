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
        rules::{
            document::{check_component_names, check_servers, check_tags},
            extensions::check_extensions,
            opaque::check_opaque,
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

        // A document is checked against the version it claims to be. 3.2-only
        // constructs are `#[cfg]`-gated, so a 3.1-only build cannot hold one --
        // but a 3.2-capable build can, and being asked to validate such a
        // document as 3.1 has to say so rather than pass a description 3.1
        // cannot express. This is the same walk `Document::emit` refuses on,
        // read here so that validating and emitting agree.
        //
        // `EmptyDocument` used to be the only rule this version was consulted
        // for, and it is raised nowhere now: every version requires a document
        // to carry at least one of `paths`, `components` or `webhooks`, and
        // `Document::paths` is always serialized, so the condition is prevented
        // by construction rather than reported after the fact.
        if !self.version.supports_3_2() {
            let blockers = crate::emit::downgrade::three_two_only_constructs(document);
            if !blockers.is_empty() {
                violations.push(Violation::error("#", SpecError::RequiresV3_2 { blockers }));
            }
        }

        // A License Object setting both `identifier` and `url` used to be
        // checked here. `License` now holds at most one of the two, so a
        // document carrying both cannot reach this function: it fails to
        // deserialize, and there is no way to build one.

        check_servers(document, &mut violations);
        check_tags(document, &mut violations);
        check_component_names(document, &mut violations);
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
