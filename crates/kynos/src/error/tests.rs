use kynos_openapi::{Severity, SpecError, Violation};

use super::Error;
use crate::middleware::contribution::ContributionConflict;

/// Two violations of different severities, which is the shape a real validation
/// run produces and the one a fixed sentence hides.
fn invalid() -> Error {
    Error::Invalid {
        violations: vec![
            Violation {
                location: "/paths/~1users/get".to_owned(),
                severity: Severity::Error,
                error: SpecError::NoResponses,
            },
            Violation {
                location: "/tags/1".to_owned(),
                severity: Severity::Warning,
                error: SpecError::DuplicateTag {
                    name: "admin".to_owned(),
                },
            },
        ],
    }
}

/// `Error::Invalid` is what all four `Router` methods return, and the violations
/// it carries are the entire content of the failure. Naming none of them leaves
/// a reader with a sentence and nothing to act on.
///
/// A set is also the one thing a cause chain cannot carry, so this has to be in
/// the message rather than behind `source()`.
#[test]
fn an_invalid_router_names_every_violation() {
    let rendered = invalid().to_string();

    assert!(
        rendered.contains("an operation must declare at least one response"),
        "the first violation is missing from `{rendered}`"
    );
    assert!(
        rendered.contains("tag `admin` is declared more than once"),
        "the second violation is missing from `{rendered}`"
    );
    assert!(
        rendered.contains("/paths/~1users/get"),
        "the first violation's location is missing from `{rendered}`"
    );
}

/// The violations are in the message, so offering the first one as a cause as
/// well would make every reporter print it twice: once in the list and once
/// under `Caused by`.
#[test]
fn an_invalid_router_offers_no_cause() {
    assert!(std::error::Error::source(&invalid()).is_none());
}

/// Every failure raised while a router is built has to have a way into
/// [`Error`], or the code that raises it has nothing to return.
///
/// `ContributionConflict` had none: `OperationContribution::merge` and
/// `OperationCx::contribute` both produce one during the build, and neither
/// could report it.
#[test]
fn a_contribution_conflict_becomes_a_build_failure() {
    let error = Error::from(ContributionConflict::DefaultResponse);

    assert!(matches!(error, Error::Contribution(_)));
}

/// `#[error(transparent)]` means the wrapper adds no words of its own, so what
/// a user reads is the conflict's own explanation rather than "the router does
/// not describe a valid API".
#[test]
fn the_conflict_speaks_for_itself() {
    let conflict = ContributionConflict::DefaultResponse;
    let expected = conflict.to_string();

    assert_eq!(
        Error::from(ContributionConflict::DefaultResponse).to_string(),
        expected
    );
}
