//! Errors, and the one recommended way to represent them on the wire.
//!
//! Kynos uses [RFC 9457 problem details] for every error it produces, and
//! `#[derive(ApiError)]` produces them for yours, so a client can handle
//! failures generically.
//!
//! This covers the framework's *own* rejections: a body that fails to parse is
//! a problem document in the operation's `responses`, because
//! [`FromRequestParts::Rejection`](crate::extract::FromRequestParts::Rejection)
//! must implement [`Responses`](crate::response::Responses).
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
    /// Every violation is named in the message, so this variant has no
    /// `source()`: a chain holds one error and validation produces a set.
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
    /// Raised while the router is built, so the conflict is caught before the
    /// service starts.
    #[error(transparent)]
    Contribution(#[from] crate::middleware::contribution::ContributionConflict),

    /// An interceptor was configured with a combination it cannot honour.
    ///
    /// Unlike [`Contribution`](Error::Contribution), this is one interceptor
    /// disagreeing with the protocol it implements, caught while the router is
    /// built.
    #[error(transparent)]
    Middleware(#[from] crate::middleware::MiddlewareError),

    /// The description could not be emitted as JSON.
    #[error("the description could not be emitted as JSON")]
    Json(#[from] serde_json::Error),

    /// The description could not be emitted as YAML.
    #[cfg(feature = "yaml")]
    #[error(transparent)]
    Yaml(#[from] kynos_openapi::emit::YamlError),

    /// The server configuration or transport failed.
    #[cfg(feature = "server")]
    #[error(transparent)]
    Server(#[from] crate::server::error::ServerError),
}

/// Lets `?` carry a TLS failure out of a `kynos::Result` function, through
/// [`Error::Server`].
#[cfg(feature = "tls")]
impl From<crate::server::tls::error::TlsError> for Error {
    fn from(error: crate::server::tls::error::TlsError) -> Self {
        Self::Server(error.into())
    }
}
