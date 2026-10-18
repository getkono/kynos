//! What it means for two declarations to disagree about one operation.
//!
//! Interceptors that disagree are refused at compile time by
//! `Router::intercept`'s bound. The conflict left is two declarations claiming
//! one security scheme name with different schemes, reported while the router
//! is built.

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
