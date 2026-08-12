//! What it means for two interceptors to disagree.
//!
//! There is no longer a contribution *value*: what an interceptor declares is
//! read from its associated types. What survives is the vocabulary for two of
//! them declaring incompatible things about one operation, which
//! `Router::intercept` rejects at compile time and which the escape hatches --
//! where the types are erased and the check cannot run -- still report while
//! the router is built.

use kynos_openapi::{ComponentName, ParameterIn, StatusPattern};

/// Two interceptors disagreed about the same part of the description.
///
/// One variant per part, rather than a free-text field, so that a caller can
/// tell a contested status from a contested parameter without parsing a
/// message.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ContributionConflict {
    /// Both declared a different response for the same status.
    #[error("two interceptors declare different responses for `{status}`")]
    Response {
        /// The contested status pattern.
        status: StatusPattern,
    },

    /// Both declared a different `default` response.
    #[error("two interceptors declare different `default` responses")]
    DefaultResponse,

    /// Both declared a different header under the same name and status.
    #[error("two interceptors declare different `{name}` headers on `{status}`")]
    ResponseHeader {
        /// The contested header name.
        name: String,
        /// The status it appears on.
        status: StatusPattern,
    },

    /// Both declared a different parameter with the same name and location.
    #[error("two interceptors declare different `in: {location}` parameters named `{name}`")]
    Parameter {
        /// The contested parameter name.
        name: String,
        /// Where it is carried.
        location: ParameterIn,
    },

    /// Both registered a different scheme under one component name.
    #[error("two interceptors register different security schemes named `{name}`")]
    SecurityScheme {
        /// The contested component name.
        name: ComponentName,
    },
}
