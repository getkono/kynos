//! The Callback Object.

use std::fmt;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, Visitor},
    ser::SerializeMap,
};

use crate::{
    Map,
    model::{extensions::Extensions, paths::item::PathItem, reference::RefOr},
};

/// Out-of-band requests the API makes as part of an operation.
///
/// Keys are runtime expressions identifying the URL to call, such as
/// `{$request.body#/callbackUrl}`. Each maps to the operations the API will
/// perform against that URL.
///
/// Extensions sit beside those keys, so this is not a bare map — see
/// [`Paths`](crate::Paths), which has the same shape for the same reason.
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
        let mut map = serializer.serialize_map(Some(self.items.len() + self.extensions.0.len()))?;
        for (expression, item) in &self.items {
            map.serialize_entry(expression, item)?;
        }
        for (key, value) in &self.extensions.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Callback {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CallbackVisitor;

        impl<'de> Visitor<'de> for CallbackVisitor {
            type Value = Callback;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of runtime expressions to path items")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Callback, A::Error> {
                let mut callback = Callback::new();
                while let Some(key) = access.next_key::<String>()? {
                    if key.starts_with(crate::model::extensions::EXTENSION_PREFIX) {
                        callback.extensions.0.insert(key, access.next_value()?);
                    } else {
                        callback.items.insert(key, access.next_value()?);
                    }
                }
                Ok(callback)
            }
        }

        deserializer.deserialize_map(CallbackVisitor)
    }
}
