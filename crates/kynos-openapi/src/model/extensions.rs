//! Specification extensions (`x-` prefixed fields).

use std::{fmt, marker::PhantomData};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, Visitor},
    ser::SerializeMap,
};
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

/// Writes `items` and then `extensions` as one flat map, as an object whose
/// keys are its entries writes itself.
pub(crate) fn serialize_with_items<S: Serializer, T: Serialize>(
    serializer: S,
    items: &Map<T>,
    extensions: &Extensions,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(items.len() + extensions.0.len()))?;
    for (key, item) in items {
        map.serialize_entry(key, item)?;
    }
    for (key, value) in &extensions.0 {
        map.serialize_entry(key, value)?;
    }
    map.end()
}

/// Reads one flat map into its entries and its `x-` extensions, the inverse
/// of [`serialize_with_items`]. `expecting` names the map in a type error.
pub(crate) fn deserialize_with_items<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
    expecting: &'static str,
) -> Result<(Map<T>, Extensions), D::Error> {
    struct ItemsVisitor<T> {
        expecting: &'static str,
        item: PhantomData<T>,
    }

    impl<'de, T: Deserialize<'de>> Visitor<'de> for ItemsVisitor<T> {
        type Value = (Map<T>, Extensions);

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.expecting)
        }

        fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            let mut items = Map::new();
            let mut extensions = Extensions::new();
            while let Some(key) = access.next_key::<String>()? {
                if key.starts_with(EXTENSION_PREFIX) {
                    extensions.0.insert(key, access.next_value()?);
                } else {
                    items.insert(key, access.next_value()?);
                }
            }
            Ok((items, extensions))
        }
    }

    deserializer.deserialize_map(ItemsVisitor {
        expecting,
        item: PhantomData,
    })
}

#[cfg(test)]
mod tests;
