//! The Paths, Path Item and Operation Objects, and path templating.

pub mod item;
pub mod method;
pub mod operation;
pub mod template;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Map,
    model::{
        extensions::{Extensions, deserialize_with_items, serialize_with_items},
        paths::{item::PathItem, template::PathTemplate},
    },
};

/// The available paths and the operations on each.
///
/// Not a bare map: the specification lets it carry extensions alongside its
/// path keys, as [`Responses`](crate::Responses) does.
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

    /// Returns `true` when the object holds neither a path item nor an
    /// extension.
    ///
    /// Extensions count, as for [`Responses::is_empty`](crate::Responses::is_empty).
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
        serialize_with_items(serializer, &self.items, &self.extensions)
    }
}

impl<'de> Deserialize<'de> for Paths {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (items, extensions) =
            deserialize_with_items(deserializer, "a map of path templates to path items")?;
        Ok(Self { items, extensions })
    }
}

#[cfg(test)]
mod tests;
