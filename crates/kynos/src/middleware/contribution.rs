//! What it means for two interceptors to disagree.
//!
//! `Router::intercept` rejects such a conflict at compile time; the escape
//! hatches, where the types are erased, report it while the router is built.

use kynos_openapi::{ComponentName, ParameterIn, StatusPattern};

/// Two interceptors disagreed about the same part of the description.
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
