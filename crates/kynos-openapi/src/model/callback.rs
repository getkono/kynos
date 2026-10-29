//! The Callback Object.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Map,
    model::{
        extensions::{Extensions, deserialize_with_items, serialize_with_items},
        paths::item::PathItem,
        reference::RefOr,
    },
};

/// Out-of-band requests the API makes as part of an operation.
///
/// Keys are runtime expressions identifying the URL to call, such as
/// `{$request.body#/callbackUrl}`. Each maps to the operations the API will
/// perform against that URL.
///
/// Extensions sit beside those keys, as in [`Paths`](crate::Paths).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Callback {
    /// The Path Items, keyed by runtime expression.
    pub items: Map<RefOr<PathItem>>,

    /// Specification extensions.
    pub extensions: Extensions,
}

impl Callback {
    /// Creates an empty callback map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares the operations performed against the URL named by `expression`.
    ///
    /// An `expression` beginning `x-` would be read back as an extension, so
    /// [`Validator`](crate::validate::Validator) refuses it with
    /// [`SpecError::CallbackKeyedAsExtension`](crate::validate::violation::SpecError::CallbackKeyedAsExtension).
    #[must_use]
    pub fn with(mut self, expression: impl Into<String>, path_item: PathItem) -> Self {
        self.items.insert(expression.into(), RefOr::Item(path_item));
        self
    }

    /// Returns `true` when nothing at all is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.extensions.is_empty()
    }
}

impl Serialize for Callback {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_with_items(serializer, &self.items, &self.extensions)
    }
}

impl<'de> Deserialize<'de> for Callback {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (items, extensions) =
            deserialize_with_items(deserializer, "a map of runtime expressions to path items")?;
        Ok(Self { items, extensions })
    }
}
