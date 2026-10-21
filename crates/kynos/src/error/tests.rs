use kynos_openapi::{ComponentName, Severity, SpecError, Violation};

use super::Error;
use crate::middleware::contribution::ContributionConflict;

/// The one conflict the registry raises, under a name the tests can spell out.
fn scheme_conflict() -> ContributionConflict {
    ContributionConflict::SecurityScheme {
        name: ComponentName::new("bearer").expect("a legal component name"),
    }
}

/// Two violations of different severities, which is the shape a real validation
/// run produces and the one a fixed sentence hides.
fn invalid() -> Error {
    Error::Invalid {
        violations: vec![
            Violation {
                location: "/paths/~1users/get".to_owned(),
                severity: Severity::Error,
                error: SpecError::NoResponses.into(),
            },
            Violation {
                location: "/tags/1".to_owned(),
                severity: Severity::Warning,
                error: SpecError::DuplicateTag {
                    name: "admin".to_owned(),
                }
                .into(),
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
/// `ContributionConflict` had none, so a security scheme name two declarations
/// claimed with different schemes, recorded during the build, could not be
/// reported.
#[test]
fn a_contribution_conflict_becomes_a_build_failure() {
    let error = Error::from(scheme_conflict());

    assert!(matches!(error, Error::Contribution(_)));
}

/// `#[error(transparent)]` means the wrapper adds no words of its own, so what
/// a user reads is the conflict's own explanation rather than "the router does
/// not describe a valid API".
#[test]
fn the_conflict_speaks_for_itself() {
    let expected = scheme_conflict().to_string();

    assert_eq!(Error::from(scheme_conflict()).to_string(), expected);
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

    use super::{Error, invalid, scheme_conflict};

    /// Every variant, named. An exhaustive match, so a variant added to
    /// [`Error`] stops this file compiling until it is given a case.
    fn variant_name(error: &Error) -> &'static str {
        match error {
            Error::Invalid { .. } => "Invalid",
            Error::Path(_) => "Path",
            Error::Schema(_) => "Schema",
            Error::KeyPattern(_) => "KeyPattern",
            Error::Contribution(_) => "Contribution",
            Error::Middleware(_) => "Middleware",
            Error::Link(_) => "Link",
            Error::Json(_) => "Json",
            #[cfg(feature = "yaml")]
            Error::Yaml(_) => "Yaml",
            #[cfg(feature = "server")]
            Error::Server(_) => "Server",
        }
    }

    /// One constructed value per variant, with the whole of what it must say.
    ///
    /// A transparent variant's sentence is its inner error's, rendered from a
    /// second value of the same inner error, so a wrapper that added words of
    /// its own would part the two.
    fn ledger() -> Vec<(Error, String)> {
        let path = || {
            kynos_openapi::PathTemplate::parse("users")
                .expect_err("a template without a leading slash")
        };
        let schema = || crate::schema::registry::SchemaConflict {
            name: "Order".to_owned(),
        };
        let key_pattern = || crate::schema::registry::UnenforceableKeyPattern {
            key: "app::Sku".to_owned(),
            pattern: "a(?=b)".to_owned(),
            reason: "lookaround".to_owned(),
        };
        let contribution = scheme_conflict;
        let middleware = || crate::middleware::MiddlewareError::CredentialedWildcardOrigin;
        let link = || crate::extract::link::UnresolvedLink::Unmounted {
            from: "create_user".to_owned(),
            target: "get_user",
        };
        #[cfg(feature = "yaml")]
        let yaml = || {
            <kynos_openapi::emit::yaml::YamlError as serde::ser::Error>::custom(
                "a number with no YAML representation",
            )
        };

        vec![
            (
                invalid(),
                "the router does not describe a valid API:\n  \
                 error at /paths/~1users/get: an operation must declare at least one response\n  \
                 warning at /tags/1: tag `admin` is declared more than once"
                    .to_owned(),
            ),
            (Error::Path(path()), path().to_string()),
            (Error::Schema(schema()), schema().to_string()),
            (
                Error::KeyPattern(Box::new(key_pattern())),
                key_pattern().to_string(),
            ),
            (
                Error::Contribution(contribution()),
                contribution().to_string(),
            ),
            (Error::Middleware(middleware()), middleware().to_string()),
            (Error::Link(Box::new(link())), link().to_string()),
            (
                Error::Json(
                    serde_json::from_str::<u8>("not a number").expect_err("a parse failure"),
                ),
                "the description could not be emitted as JSON".to_owned(),
            ),
            #[cfg(feature = "yaml")]
            (Error::Yaml(yaml()), yaml().to_string()),
            #[cfg(feature = "server")]
            (
                Error::Server(crate::server::error::ServerError::NoListeners),
                crate::server::error::ServerError::NoListeners.to_string(),
            ),
        ]
    }

    #[test]
    fn each_variant_says_what_failed() {
        for (error, expected) in ledger() {
            let name = variant_name(&error);

            assert_eq!(error.to_string(), expected, "{name}");
        }
    }

    /// What the transparent variants' inner errors say, so that rendering a
    /// second value of each in the ledger is not the only thing pinning the
    /// sentence a caller reads.
    #[test]
    fn each_inner_error_says_what_failed() {
        assert_eq!(
            Error::Path(
                kynos_openapi::PathTemplate::parse("users")
                    .expect_err("a template without a leading slash")
            )
            .to_string(),
            "path template `users` must begin with `/`"
        );
        assert_eq!(
            Error::Schema(crate::schema::registry::SchemaConflict {
                name: "Order".to_owned(),
            })
            .to_string(),
            "component name `Order` is claimed by two structurally different schemas; rename one \
             of the Rust types, or implement `Schema` by hand for one and return a different \
             `name()`"
        );
        assert_eq!(
            Error::KeyPattern(Box::new(crate::schema::registry::UnenforceableKeyPattern {
                key: "app::Sku".to_owned(),
                pattern: "a(?=b)".to_owned(),
                reason: "lookaround".to_owned(),
            }))
            .to_string(),
            "the map key `app::Sku` declares the pattern `a(?=b)`, which cannot be enforced: \
             lookaround"
        );
        assert_eq!(
            Error::Contribution(scheme_conflict()).to_string(),
            "two different security schemes are registered under the name `bearer`"
        );
        assert_eq!(
            Error::Middleware(crate::middleware::MiddlewareError::CredentialedWildcardOrigin)
                .to_string(),
            "a CORS configuration permits any origin and also permits credentials, which the \
             protocol forbids; drop `allow_credentials`, or replace `allow_any_origin` with the \
             origins `allow_origins` should name"
        );
        assert_eq!(
            Error::Link(Box::new(crate::extract::link::UnresolvedLink::Unmounted {
                from: "create_user".to_owned(),
                target: "get_user",
            }))
            .to_string(),
            "operation `create_user` links to `get_user`, which this router does not mount"
        );
        #[cfg(feature = "server")]
        assert_eq!(
            Error::Server(crate::server::error::ServerError::NoListeners).to_string(),
            "the server has no listeners"
        );
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
            "KeyPattern",
            "Contribution",
            "Middleware",
            "Link",
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
