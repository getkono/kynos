use super::Error;
use crate::middleware::contribution::ContributionConflict;

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
