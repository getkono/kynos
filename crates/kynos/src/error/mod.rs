//! Errors, and the one recommended way to represent them on the wire.
//!
//! Kynos uses [RFC 9457 problem details] for every error it produces, and
//! `#[derive(ApiError)]` produces them for yours. One shape across the whole
//! description means a client can handle failures generically instead of
//! learning a different envelope per endpoint.
//!
//! Crucially, this covers the framework's *own* rejections. When a body fails
//! to parse, or a path parameter will not deserialize, the resulting 400 is a
//! problem document and it appears in the operation's `responses` — because
//! [`FromRequestParts::Rejection`](crate::extract::FromRequestParts::Rejection)
//! is required to describe itself. No other Rust framework documents its
//! extractor rejections at all.
//!
//! [RFC 9457 problem details]: https://www.rfc-editor.org/rfc/rfc9457
//!
//! # How this module is laid out
//!
//! [`Error`] is the framework's own build-time failure and lives here.
//! [`problem`] holds the wire representation every error takes, and
//! [`rejection`] the ways a built-in extractor can fail.

pub mod problem;
pub mod rejection;

#[cfg(test)]
mod tests;

/// The result type used throughout the framework.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A failure raised by the framework itself, not by a handler.
///
/// These surface while a router is being built or a server started — never
/// while serving a request, where a [`Problem`](problem::Problem) is returned instead.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The router describes an API that OpenAPI cannot express, or expresses
    /// incorrectly.
    ///
    /// Every violation is named in the message rather than offered as a cause:
    /// `source()` carries one error and a validation run produces a set, so a
    /// chain cannot hold them. This variant has no cause for that reason, which
    /// also keeps a reporter from printing the first violation twice.
    #[error(
        "the router does not describe a valid API:\n{}",
        violations.iter().map(|violation| format!("  {violation}")).collect::<Vec<_>>().join("\n")
    )]
    Invalid {
        /// Every violation found, most structural first.
        violations: Vec<kynos_openapi::Violation>,
    },

    /// A path template was malformed, or collided with another.
    #[error(transparent)]
    Path(#[from] kynos_openapi::model::paths::template::InvalidPathTemplate),

    /// Two types claimed the same component name.
    #[error(transparent)]
    Schema(#[from] crate::schema::registry::SchemaConflict),

    /// Two interceptors covering one operation disagreed about what they
    /// contribute to it.
    ///
    /// Raised while the router is built, which is the whole point: two layers
    /// that disagree about what a 429 means are caught before the service
    /// starts rather than in production.
    #[error(transparent)]
    Contribution(#[from] crate::middleware::contribution::ContributionConflict),

    /// The description could not be emitted as JSON.
    ///
    /// Named after the emitter rather than after serialization in general: the
    /// conversion is what records which one failed, so a caller reading the
    /// message does not have to work out which of a document's two encodings
    /// was in play.
    #[error("the description could not be emitted as JSON")]
    Json(#[from] serde_json::Error),

    /// The description could not be emitted as YAML.
    #[cfg(feature = "yaml")]
    #[error("the description could not be emitted as YAML")]
    Yaml(#[from] serde_yaml_ng::Error),

    /// The server configuration or transport failed.
    #[cfg(feature = "server")]
    #[error(transparent)]
    Server(#[from] crate::server::error::ServerError),
}
