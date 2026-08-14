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

/// `From` is not transitive. Both hops existed -- `TlsError` into `ServerError`
/// and `ServerError` into `Error` -- and `?` still refused a TLS failure in a
/// `kynos::Result` function, which is why the example had to name the
/// intermediate type by hand.
#[cfg(feature = "tls")]
#[test]
fn a_tls_failure_becomes_a_build_failure() {
    use crate::{server::error::ServerError, server::tls::error::TlsError};

    let error = Error::from(TlsError::ZeroHandshakeTimeout);

    assert!(matches!(error, Error::Server(ServerError::Tls(_))));
}

/// Both hops are `#[error(transparent)]`, so travelling two of them adds no
/// words: what a reader gets is the TLS failure's own sentence rather than
/// "the server could not start".
#[cfg(feature = "tls")]
#[test]
fn a_tls_failure_still_speaks_for_itself() {
    use crate::server::tls::error::TlsError;

    assert_eq!(
        Error::from(TlsError::ZeroHandshakeTimeout).to_string(),
        TlsError::ZeroHandshakeTimeout.to_string()
    );
}

/// One case per variant of [`Error`], the framework's own build-time failure.
///
/// Three of the seven were reached before this: `Invalid`, `Contribution` and
/// `Server`. The other four had no test at all, which for `Path`, `Schema` and
/// the two emitters means nothing checked that they render anything a caller
/// could act on -- `reporting.rs` proves the *type* is reportable, not that a
/// variant of it says something.
mod variants {
    use std::collections::BTreeSet;

    use super::{Error, invalid};

    /// Every variant, named. An exhaustive match, so a variant added to
    /// [`Error`] stops this file compiling until it is given a case.
    fn variant_name(error: &Error) -> &'static str {
        match error {
            Error::Invalid { .. } => "Invalid",
            Error::Path(_) => "Path",
            Error::Schema(_) => "Schema",
            Error::Contribution(_) => "Contribution",
            Error::Json(_) => "Json",
            #[cfg(feature = "yaml")]
            Error::Yaml(_) => "Yaml",
            #[cfg(feature = "server")]
            Error::Server(_) => "Server",
        }
    }

    /// One constructed value per variant, with a fragment of what it must say.
    fn ledger() -> Vec<(Error, &'static str)> {
        vec![
            (invalid(), "does not describe a valid API"),
            (
                Error::Path(
                    kynos_openapi::PathTemplate::parse("users")
                        .expect_err("a template without a leading slash"),
                ),
                "/",
            ),
            (
                Error::Schema(crate::schema::registry::SchemaConflict {
                    name: "Order".to_owned(),
                }),
                "`Order` is claimed by two structurally different schemas",
            ),
            (
                Error::Contribution(
                    crate::middleware::contribution::ContributionConflict::DefaultResponse,
                ),
                "two interceptors declare different `default` responses",
            ),
            (
                Error::Json(
                    serde_json::from_str::<u8>("not a number").expect_err("a parse failure"),
                ),
                "could not be emitted as JSON",
            ),
            #[cfg(feature = "yaml")]
            (
                Error::Yaml(serde_yaml_ng::from_str::<u8>("[").expect_err("a parse failure")),
                "could not be emitted as YAML",
            ),
            #[cfg(feature = "server")]
            (
                Error::Server(crate::server::error::ServerError::NoListeners),
                "listener",
            ),
        ]
    }

    #[test]
    fn each_variant_says_what_failed() {
        for (error, expected) in ledger() {
            let name = variant_name(&error);
            let rendered = error.to_string();

            assert!(
                rendered.contains(expected),
                "{name}: expected a message containing {expected:?}, got {rendered:?}"
            );
        }
    }

    /// The ledger against the variants, transcribed.
    ///
    /// The exhaustive match above catches a variant added without a *name*; it
    /// cannot catch one added without a constructed value, because a match arm
    /// nothing reaches still compiles.
    #[test]
    fn every_variant_has_a_case() {
        let covered: BTreeSet<&str> = ledger()
            .iter()
            .map(|(error, _)| variant_name(error))
            .collect();
        let declared: BTreeSet<&str> = [
            "Invalid",
            "Path",
            "Schema",
            "Contribution",
            "Json",
            #[cfg(feature = "yaml")]
            "Yaml",
            #[cfg(feature = "server")]
            "Server",
        ]
        .into_iter()
        .collect();

        assert_eq!(covered, declared);
    }
}
