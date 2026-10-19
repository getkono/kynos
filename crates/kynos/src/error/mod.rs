//! Errors, and the one recommended way to represent them on the wire.
//!
//! Kynos uses [RFC 9457 problem details] for every error it produces, and
//! `#[derive(ApiError)]` produces them for yours, so a client can handle
//! failures generically.
//!
//! This covers the framework's *own* rejections: a body that fails to parse is
//! a problem document in the operation's `responses`, because every
//! extractor's rejection must implement
//! [`Responses`](crate::response::Responses) — see
//! [Rejections describe themselves too](crate::extract#rejections-describe-themselves-too).
//!
//! [RFC 9457 problem details]: https://www.rfc-editor.org/rfc/rfc9457
//!
//! # How this module is laid out
//!
//! [`Error`] is the framework's own build-time failure and lives here, beside
//! [`RouterError`], what each violation it reports says is wrong.
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
/// These surface while a router is built or a server runs — never as the
/// answer to a request, where a [`Problem`](problem::Problem) is returned instead.
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
        violations: Vec<kynos_openapi::Violation<RouterError>>,
    },

    /// A path template was malformed, or collided with another.
    #[error(transparent)]
    Path(#[from] kynos_openapi::model::paths::template::InvalidPathTemplate),

    /// Two types claimed the same component name.
    #[error(transparent)]
    Schema(#[from] crate::schema::registry::SchemaConflict),

    /// A map key type declared a `pattern` no check could enforce: one that
    /// does not translate from ECMA-262, or any pattern without the `pattern`
    /// feature.
    ///
    /// A field's pattern is refused by the derive at compile time; a key's is
    /// a run-time value, so this is the earliest it can be.
    ///
    /// Boxed, so a refusal that names three strings does not widen every
    /// `kynos::Result`.
    #[error(transparent)]
    KeyPattern(#[from] Box<crate::schema::registry::UnenforceableKeyPattern>),

    /// Two declarations claimed one security scheme name with different
    /// schemes.
    ///
    /// Raised while the router is built, so the conflict is caught before the
    /// service starts.
    #[error(transparent)]
    Contribution(#[from] crate::middleware::contribution::ContributionConflict),

    /// An interceptor was configured with a combination it cannot honour.
    ///
    /// This is one interceptor disagreeing with the protocol it implements,
    /// caught while the router is built.
    #[error(transparent)]
    Middleware(#[from] crate::middleware::MiddlewareError),

    /// An operation extracts a [`Link`](crate::extract::link::Link) to an
    /// operation the router cannot render a URI for.
    ///
    /// Boxed, for [`KeyPattern`](Error::KeyPattern)'s reason.
    #[error(transparent)]
    Link(#[from] Box<crate::extract::link::UnresolvedLink>),

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

/// What is wrong with a router, as one of its
/// [`Violation`](kynos_openapi::Violation)s says.
///
/// A router is checked twice over: its description against the OpenAPI
/// specification, which reports a [`SpecError`](kynos_openapi::SpecError), and
/// itself, for what no document can hold — its match table, its interceptors,
/// the routers it absorbed. One report carries both, so a violation found in
/// the description arrives wrapped in [`Spec`](RouterError::Spec).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RouterError {
    /// The description fails to conform to the specification.
    #[error(transparent)]
    Spec(#[from] kynos_openapi::SpecError),

    /// One method on one path was described more than once.
    ///
    /// A Path Item holds one Operation per method, so no document can carry
    /// the second: the router reports it while it assembles the description.
    #[error(
        "operation `{method} {path}` is described more than once; a path item holds one \
         operation per method"
    )]
    DuplicateOperation {
        /// The method described twice.
        method: kynos_openapi::Method,
        /// The `paths` key it was described under.
        path: String,
    },

    /// An interceptor's short circuit declared statuses its responses do not
    /// describe, or the reverse.
    ///
    /// Only reachable from a hand-written
    /// [`ShortCircuit`](crate::response::ShortCircuit); the one the `ApiError`
    /// derive emits comes from the same declaration as the responses and
    /// cannot disagree with them.
    #[error(
        "`{name}` declares statuses {declared:?} as a short circuit but describes {described:?}"
    )]
    ShortCircuitMismatch {
        /// The short-circuit type.
        name: String,
        /// What its `STATUSES` const claimed.
        declared: Vec<u16>,
        /// What its `Responses` actually described.
        described: Vec<u16>,
    },

    /// Two routes cannot share the router's match table, which has no rule for
    /// which of them a request reaches — `/{x}.txt` beside `/v{x}`, say.
    ///
    /// A document may hold both; only the table refuses them.
    #[error("`{pattern}` conflicts with `{existing}` in the router's match table")]
    RouteConflict {
        /// The pattern being added.
        pattern: String,
        /// The pattern already in the table.
        existing: String,
    },

    /// A matching pattern is not one the router's match table can read.
    ///
    /// Reachable only for a pattern given verbatim, such as an unchecked
    /// route's — a catch-all before the end, say. A path template is checked
    /// when it is parsed.
    #[error("`{pattern}` is not a pattern the router's match table accepts")]
    InvalidRoutePattern {
        /// The router's matching pattern, verbatim.
        pattern: String,
    },

    /// A router that was nested or merged into another set an
    /// application-wide policy, which only the root router's applies.
    ///
    /// Reported once per policy set and located at the root: the absorbed
    /// router's value would otherwise be dropped without a word.
    #[error(
        "a nested or merged router sets `{policy}`, which only the root router's applies; \
         set it on the root router instead"
    )]
    NestedRouterPolicy {
        /// The setter the absorbed router called, as the router spells it
        /// (`trusted_proxies`).
        policy: &'static str,
    },
}

/// Lets `?` carry a TLS failure out of a `kynos::Result` function, through
/// [`Error::Server`].
#[cfg(feature = "tls")]
impl From<crate::server::tls::error::TlsError> for Error {
    fn from(error: crate::server::tls::error::TlsError) -> Self {
        Self::Server(error.into())
    }
}
