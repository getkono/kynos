//! What it means for two contributions to the description to disagree.
//!
//! Interceptors cannot: `Router::intercept` rejects two that claim one status
//! or header at compile time. What remains is a component name, which
//! [`Router::security_scheme`](crate::router::Router::security_scheme) and
//! [`OperationCx::add_security_scheme`](crate::router::operation::OperationCx::add_security_scheme)
//! may both claim, and which the router reports while it is built.

use kynos_openapi::ComponentName;

/// Two contributions to the description disagreed about the same part of it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ContributionConflict {
    /// Two different security schemes were registered under one component name.
    #[error("two different security schemes are registered under the name `{name}`")]
    SecurityScheme {
        /// The contested component name.
        name: ComponentName,
    },
}
