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
    #[error("the router does not describe a valid API")]
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

    /// The document could not be serialized.
    #[error("the description could not be serialized")]
    Serialize(#[from] serde_json::Error),

    /// The listener could not be bound, or the server could not start.
    #[error("the server could not start")]
    Io(#[from] std::io::Error),

    /// The server configuration or transport failed.
    #[cfg(feature = "server")]
    #[error(transparent)]
    Server(#[from] crate::server::error::ServerError),
}
