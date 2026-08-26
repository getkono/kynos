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
/// Most objects in the model carry one of these flattened into their
/// serialization. Two do not, because the specification forbids it: the
/// Reference Object and the Security Requirement Object.
///
/// Keys are *not* checked on construction — [`crate::validate`] reports
/// non-conforming names, so that a description parsed from an external source
/// round-trips rather than being silently rewritten.
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
    /// Removal preserves the order of the remaining entries, which is what
    /// keeps an emitted description byte-stable across an edit. Owning that
    /// choice here is the point: a caller reaching through to the map would
    /// have to make it, and could make it differently each time.
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
mod tests {
    use super::Extensions;

    #[test]
    fn extension_names_require_the_x_prefix() {
        assert!(Extensions::is_valid_name("x-internal-id"));
        assert!(!Extensions::is_valid_name("internal-id"));
    }

    #[test]
    fn oai_reserved_prefixes_are_rejected() {
        assert!(!Extensions::is_valid_name("x-oai-anything"));
        assert!(!Extensions::is_valid_name("x-oas-anything"));
    }
}
