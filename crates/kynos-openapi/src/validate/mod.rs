//! Structural validation of a [`Document`].
//!
//! Nearly everything checked here is a rule the OpenAPI specification states
//! but that the type system cannot enforce — uniqueness across a whole
//! document, correspondence between a path template and its parameters, names
//! that must resolve. Mutual exclusions between fields are spelled as types
//! instead. The exceptions are Kynos's own policies, stricter than the
//! specification — [`SpecError::NoResponses`] is one, and says why.
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
            content::check_component_content,
            document::{
                check_component_names, check_component_parameters, check_servers, check_tags,
            },
            extensions::check_extensions,
            opaque::check_opaque,
            operations::check_link_targets,
            schemas::check_schemas,
            security::check_security_schemes,
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

        // Validating as 3.1 refuses what 3.1 cannot express by the same walk
        // `Document::emit` refuses on, so validating and emitting agree. A
        // requirement naming a scheme by URI is the security rules' to report,
        // as an undeclared scheme, by the same predicate the walk applies.
        // Validating as 3.2 refuses the walk's unrecognised fields alone, since
        // both versions require an extension's name to begin with `x-`.
        if self.version.supports_3_2() {
            for field in crate::emit::downgrade::unrecognised_fields(document) {
                let (location, name) = holder_and_key(&field);
                violations.push(Violation::error(
                    location,
                    SpecError::InvalidExtensionName { name },
                ));
            }
        } else {
            let obstacles = crate::emit::downgrade::three_one_obstacles(document);
            if !obstacles.fields.is_empty() {
                violations.push(Violation::error(
                    "#",
                    SpecError::RequiresV3_2 {
                        blockers: obstacles.fields,
                    },
                ));
            }
            for response in obstacles.undescribed_responses {
                violations.push(Violation::error(
                    response,
                    SpecError::MissingResponseDescription,
                ));
            }
        }

        check_servers(document, &mut violations);
        check_tags(document, &mut violations);
        check_component_names(document, &mut violations);
        check_component_parameters(document, &mut violations);
        check_component_content(document, &mut violations);
        check_security_schemes(document, &mut violations);
        self.check_security(document, &mut violations);
        self.check_paths(document, &mut violations);
        check_link_targets(document, &mut violations);
        check_schemas(document, self.version, &mut violations);
        check_opaque(document, &mut violations);
        check_extensions("#", &document.extensions, &mut violations);

        violations
    }
}

/// Splits a field's pointer into its holding object's pointer and the field's
/// name, unescaped: the location and name every extension-name violation uses.
fn holder_and_key(pointer: &str) -> (&str, String) {
    let (holder, token) = pointer.rsplit_once('/').unwrap_or(("#", pointer));
    (holder, token.replace("~1", "/").replace("~0", "~"))
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
