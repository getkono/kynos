//! Properties over a whole generated document.
//!
//! Round-tripping, determinism, the totality of validation, and the two
//! emission rules. The path-template properties are in `templates.rs`; the
//! generators both files draw on are in `support/`.

use kynos_openapi::{
    Document, Severity, SpecError, SpecVersion, Violation,
    emit::downgrade::three_two_only_constructs, validate::Validator,
};
use proptest::prelude::*;

#[path = "support/mod.rs"]
mod support;
use support::*;

// --- Helpers used by the properties -------------------------------------

fn to_json(document: &Document) -> String {
    document
        .to_json()
        .expect("every generated value is representable in JSON")
}

fn parse(json: &str) -> Document {
    serde_json::from_str(json).expect("what the model emits, the model reads")
}

/// Violations as a sorted multiset of their rendered form.
///
/// Two rules collect their inputs through a `HashSet`, so the order of the
/// violations they emit is not fixed; the set of them is what the caller acts
/// on.
fn rendered(violations: &[Violation]) -> Vec<String> {
    let mut rendered: Vec<String> = violations.iter().map(ToString::to_string).collect();
    rendered.sort();
    rendered
}

/// The rendered form of the error-severity violations only.
fn rendered_errors(violations: &[Violation]) -> Vec<String> {
    let mut rendered: Vec<String> = violations
        .iter()
        .filter(|violation| violation.severity == Severity::Error)
        .map(ToString::to_string)
        .collect();
    rendered.sort();
    rendered
}

// --- Properties ----------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Parsing what was emitted yields the document that was emitted.
    #[test]
    fn a_document_survives_a_json_round_trip(document in arb_document()) {
        prop_assert_eq!(parse(&to_json(&document)), document);
    }

    /// Serialization depends on nothing but the value.
    #[test]
    fn serialization_is_deterministic(document in arb_document()) {
        let json = to_json(&document);
        prop_assert_eq!(to_json(&document.clone()), json.clone());
        prop_assert_eq!(to_json(&parse(&json)), json);
    }

    /// Validation terminates and reports the same thing every time, at every
    /// specification version, for any document at all.
    #[test]
    fn validation_is_total(document in arb_document()) {
        for &version in VERSIONS {
            let validator = Validator::new(version);
            let violations = validator.validate(&document);
            prop_assert_eq!(rendered(&validator.validate(&document)), rendered(&violations));

            let errors = document.validate(version).err().unwrap_or_default();
            prop_assert_eq!(rendered(&errors), rendered_errors(&violations));

            for violation in &violations {
                prop_assert!(violation.location.starts_with('#'));
            }
        }
    }

    /// Emitting as 3.1 fails exactly when a 3.2-only construct is in the way,
    /// and emitting at a fixed version is idempotent.
    #[test]
    fn emitting_refuses_a_lossy_downgrade(document in arb_document()) {
        let blockers = three_two_only_constructs(&document);
        match document.emit(SpecVersion::V3_1) {
            Ok(emitted) => {
                prop_assert!(blockers.is_empty());
                prop_assert_eq!(&emitted.openapi, "3.1.2");
                prop_assert_eq!(emitted.emit(SpecVersion::V3_1).ok(), Some(emitted.clone()));
                prop_assert_eq!(three_two_only_constructs(&emitted), blockers);
            }
            Err(error) => {
                prop_assert!(!blockers.is_empty());
                prop_assert_eq!(error, SpecError::RequiresV3_2 { blockers });
            }
        }
    }

    /// A document is emittable as the version it already declares.
    #[cfg(feature = "openapi32")]
    #[test]
    fn emitting_as_the_newer_version_always_succeeds(document in arb_document()) {
        let emitted = document.emit(SpecVersion::V3_2).expect("3.2 expresses everything");
        prop_assert_eq!(&emitted.openapi, "3.2.0");
        prop_assert_eq!(emitted.emit(SpecVersion::V3_2).ok(), Some(emitted.clone()));
        prop_assert_eq!(emitted.spec_version(), Some(SpecVersion::V3_2));
    }
}
