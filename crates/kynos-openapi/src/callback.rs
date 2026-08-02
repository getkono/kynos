//! The Callback Object.

use serde::{Deserialize, Serialize};

use crate::{Map, paths::PathItem, reference::RefOr};

/// Out-of-band requests the API makes as part of an operation.
///
/// Keys are runtime expressions identifying the URL to call, such as
/// `{$request.body#/callbackUrl}`. Each maps to the operations the API will
/// perform against that URL.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Callback(pub Map<RefOr<PathItem>>);

impl Callback {
    /// Creates an empty callback map.
    #[must_use]
    pub fn new() -> Self {
        Self(Map::new())
    }

    /// Declares the operations performed against the URL named by `expression`.
    #[must_use]
    pub fn with(mut self, expression: impl Into<String>, path_item: PathItem) -> Self {
        self.0.insert(expression.into(), RefOr::Item(path_item));
        self
    }

    /// Returns `true` when no callback is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
