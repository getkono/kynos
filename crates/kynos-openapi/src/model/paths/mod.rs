//! The Paths, Path Item and Operation Objects, and path templating.

pub mod item;
pub mod method;
pub mod operation;
pub mod template;

use serde::{Deserialize, Serialize};

use crate::{
    Map,
    model::paths::{item::PathItem, template::PathTemplate},
};

/// The available paths and the operations on each.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Paths(pub Map<PathItem>);

impl Paths {
    /// Creates an empty path map.
    #[must_use]
    pub fn new() -> Self {
        Self(Map::new())
    }

    /// Inserts a path item, replacing any existing entry for that template.
    pub fn insert(&mut self, template: &PathTemplate, item: PathItem) -> Option<PathItem> {
        self.0.insert(template.as_str().to_owned(), item)
    }

    /// Looks up the path item for a template.
    #[must_use]
    pub fn get(&self, template: &PathTemplate) -> Option<&PathItem> {
        self.0.get(template.as_str())
    }

    /// Returns `true` when no path is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests;
