//! Specification extensions (`x-` prefixed fields).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Map;

/// The prefix every specification extension field name must carry.
pub const EXTENSION_PREFIX: &str = "x-";

/// Prefixes reserved by the OpenAPI Initiative.
///
/// A description that is not itself an OAI publication must not use these.
pub const RESERVED_EXTENSION_PREFIXES: &[&str] = &["x-oai-", "x-oas-"];

/// Implementation-defined fields attached to an object.
///
/// Flattened into most objects; the specification forbids them on the
/// Reference Object and the Security Requirement Object.
///
/// Keys are *not* checked on construction, so parsed descriptions round-trip;
/// [`crate::validate`] reports non-conforming names.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Extensions(pub Map<Value>);

impl Extensions {
    /// Creates an empty set of extensions.
    #[must_use]
    pub fn new() -> Self {
        Self(Map::new())
    }

    /// Returns `true` when no extension is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Inserts an extension, returning the previous value for that key.
    ///
    /// The `x-` prefix is not added for you; pass the full field name.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) -> Option<Value> {
        self.0.insert(key.into(), value.into())
    }

    /// Looks up an extension by its full field name.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Removes an extension, returning its value.
    ///
    /// Preserves the order of the remaining entries, keeping emitted output
    /// byte-stable across an edit.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.0.shift_remove(key)
    }

    /// Returns `true` when `name` is a well-formed extension field name that is
    /// not reserved by the OpenAPI Initiative.
    #[must_use]
    pub fn is_valid_name(name: &str) -> bool {
        name.starts_with(EXTENSION_PREFIX)
            && !RESERVED_EXTENSION_PREFIXES
                .iter()
                .any(|reserved| name.starts_with(reserved))
    }
}

#[cfg(test)]
mod tests;
