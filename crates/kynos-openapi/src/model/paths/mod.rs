//! The Paths, Path Item and Operation Objects, and path templating.

pub mod item;
pub mod method;
pub mod operation;
pub mod template;

use std::fmt;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, Visitor},
    ser::SerializeMap,
};

use crate::{
    Map,
    model::{
        extensions::Extensions,
        paths::{item::PathItem, template::PathTemplate},
    },
};

/// The available paths and the operations on each.
///
/// The specification lets this object carry extensions alongside its path
/// keys, so it is not a bare map: a `#[serde(transparent)]` newtype made an
/// `x-` member whose value was not a Path Item fail to parse outright. The
/// shape is [`Responses`](crate::Responses)' — patterned keys, extensions, and
/// a hand-written (de)serializer to tell them apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paths {
    /// The Path Items, keyed by path template.
    pub items: Map<PathItem>,

    /// Specification extensions.
    pub extensions: Extensions,
}

impl Paths {
    /// Creates an empty path map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a path item, replacing any existing entry for that template.
    pub fn insert(&mut self, template: &PathTemplate, item: PathItem) -> Option<PathItem> {
        self.items.insert(template.as_str().to_owned(), item)
    }

    /// Looks up the path item for a template.
    #[must_use]
    pub fn get(&self, template: &PathTemplate) -> Option<&PathItem> {
        self.items.get(template.as_str())
    }

    /// Returns `true` when nothing at all is declared.
    ///
    /// Extensions count, for the reason
    /// [`Responses::is_empty`](crate::Responses::is_empty)'s do: a Paths
    /// Object carrying only `x-` fields still has something to write down.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.extensions.is_empty()
    }

    /// Returns `true` when a path is declared.
    #[must_use]
    pub fn declares_a_path(&self) -> bool {
        !self.items.is_empty()
    }
}

impl Serialize for Paths {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.items.len() + self.extensions.0.len()))?;
        for (key, item) in &self.items {
            map.serialize_entry(key, item)?;
        }
        for (key, value) in &self.extensions.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Paths {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PathsVisitor;

        impl<'de> Visitor<'de> for PathsVisitor {
            type Value = Paths;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of path templates to path items")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Paths, A::Error> {
                let mut paths = Paths::new();
                while let Some(key) = access.next_key::<String>()? {
                    if key.starts_with(crate::model::extensions::EXTENSION_PREFIX) {
                        paths.extensions.0.insert(key, access.next_value()?);
                    } else {
                        paths.items.insert(key, access.next_value()?);
                    }
                }
                Ok(paths)
            }
        }

        deserializer.deserialize_map(PathsVisitor)
    }
}

#[cfg(test)]
mod tests;
