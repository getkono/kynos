//! The Security Requirement Object.

use serde::{Deserialize, Serialize};

use crate::Map;

/// The security schemes that must be satisfied to invoke an operation.
///
/// Each key names a scheme in
/// [`Components::security_schemes`](crate::Components::security_schemes); the
/// value lists the required scopes, which is meaningful only for `oauth2` and
/// `openIdConnect`. All entries in one requirement must be satisfied together;
/// a list of requirements is satisfied when any one of them is.
///
/// An empty requirement means anonymous access is permitted.
///
/// This object carries no extensions: the specification does not permit them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecurityRequirement(pub Map<Vec<String>>);

impl SecurityRequirement {
    /// Creates a requirement permitting anonymous access.
    #[must_use]
    pub fn anonymous() -> Self {
        Self(Map::new())
    }

    /// Requires a scheme that takes no scopes.
    pub fn scheme(name: impl Into<String>) -> Self {
        let mut map = Map::new();
        map.insert(name.into(), Vec::new());
        Self(map)
    }

    /// Requires a scheme together with a set of scopes.
    pub fn scoped(
        name: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let mut map = Map::new();
        map.insert(name.into(), scopes.into_iter().map(Into::into).collect());
        Self(map)
    }

    /// Adds another scheme that must be satisfied alongside the existing ones.
    #[must_use]
    pub fn and(
        mut self,
        name: impl Into<String>,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.0
            .insert(name.into(), scopes.into_iter().map(Into::into).collect());
        self
    }

    /// Returns `true` when this requirement permits anonymous access.
    #[must_use]
    pub fn is_anonymous(&self) -> bool {
        self.0.is_empty()
    }
}
