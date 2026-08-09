//! The Discriminator Object.

use serde::{Deserialize, Serialize};

use crate::Map;

/// Polymorphism support: which subschema applies, chosen by a payload property.
///
/// A discriminator is only meaningful next to `oneOf`, `anyOf` or `allOf`, and
/// must not change whether an instance validates — it only makes the choice
/// cheaper to determine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discriminator {
    /// The name of the property holding the discriminating value.
    #[serde(rename = "propertyName")]
    pub property_name: String,

    /// An explicit mapping from discriminating value to schema name or URI.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub mapping: Map<String>,

    /// The schema to use when the discriminating property is absent, or holds a
    /// value with no mapping.
    ///
    /// Introduced in OpenAPI 3.2. Required whenever the discriminating property
    /// is optional — which is why a Rust enum with a `#[serde(other)]`
    /// catch-all variant cannot be described under 3.1 alone.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "defaultMapping",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_mapping: Option<String>,
}

impl Discriminator {
    /// Creates a discriminator keyed on the given property.
    pub fn new(property_name: impl Into<String>) -> Self {
        Self {
            property_name: property_name.into(),
            ..Self::default()
        }
    }

    /// Maps a discriminating value to a schema name or URI.
    #[must_use]
    pub fn with_mapping(mut self, value: impl Into<String>, schema: impl Into<String>) -> Self {
        self.mapping.insert(value.into(), schema.into());
        self
    }
}
