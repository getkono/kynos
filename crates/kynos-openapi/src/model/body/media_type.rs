//! The Media Type Object.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::encoding::Encoding, example::Example, extensions::Extensions, reference::RefOr,
        schema::Schema,
    },
};

/// Media types whose payload is a sequence of items rather than one value.
///
/// Introduced as a concept in OpenAPI 3.2, which also supplies
/// [`MediaType::item_schema`] to describe the individual items. Under 3.1 these
/// payloads can only be described as opaque strings, which is why Kynos gates
/// its streaming response types behind `openapi32`.
#[cfg(feature = "openapi32")]
pub const SEQUENTIAL_MEDIA_TYPES: &[&str] = &[
    "application/jsonl",
    "application/x-ndjson",
    "application/json-seq",
    "application/geo+json-seq",
    "text/event-stream",
    "multipart/mixed",
];

/// One representation of a request or response body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaType {
    /// The schema of the complete content.
    ///
    /// For a [sequential media type](SEQUENTIAL_MEDIA_TYPES) this describes the
    /// whole stream treated as an array, which is only useful to a consumer
    /// willing to buffer it. Use [`item_schema`](MediaType::item_schema) to
    /// describe items that are processed as they arrive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Schema>,

    /// The schema of each item within a sequential media type.
    ///
    /// Introduced in OpenAPI 3.2. This is what makes Server-Sent Events, JSON
    /// Lines and JSON Text Sequences describable at all; it may be used
    /// alongside [`schema`](MediaType::schema).
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "itemSchema",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub item_schema: Option<Schema>,

    /// A single example of the body.
    ///
    /// Mutually exclusive with [`examples`](MediaType::examples).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,

    /// Examples of the body.
    ///
    /// Mutually exclusive with [`example`](MediaType::example).
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub examples: Map<RefOr<Example>>,

    /// Encoding information for named properties.
    ///
    /// Applies only to `multipart` and `application/x-www-form-urlencoded`
    /// bodies, and only for keys that exist as properties of the schema. Must
    /// not be combined with [`prefix_encoding`](MediaType::prefix_encoding) or
    /// [`item_encoding`](MediaType::item_encoding).
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub encoding: Map<Encoding>,

    /// Positional encoding information for the leading array items.
    ///
    /// Introduced in OpenAPI 3.2, for `multipart` bodies with a fixed part
    /// order. Requires an array [`schema`](MediaType::schema) or an
    /// [`item_schema`](MediaType::item_schema).
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "prefixEncoding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub prefix_encoding: Option<Vec<Encoding>>,

    /// Encoding information applied to every remaining array item.
    ///
    /// Introduced in OpenAPI 3.2. Together with
    /// [`item_schema`](MediaType::item_schema) this describes streaming
    /// `multipart` content.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "itemEncoding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub item_encoding: Option<Box<Encoding>>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl MediaType {
    /// Describes a body by the schema of its complete content.
    #[must_use]
    pub fn new(schema: Schema) -> Self {
        Self {
            schema: Some(schema),
            ..Self::default()
        }
    }

    /// Describes a sequential body by the schema of each item.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[must_use]
    pub fn sequential(item_schema: Schema) -> Self {
        Self {
            item_schema: Some(item_schema),
            ..Self::default()
        }
    }

    /// Attaches encoding information for a named property.
    #[must_use]
    pub fn with_encoding(mut self, property: impl Into<String>, encoding: Encoding) -> Self {
        self.encoding.insert(property.into(), encoding);
        self
    }

    /// Adds a named example.
    #[must_use]
    pub fn with_example(mut self, name: impl Into<String>, example: Example) -> Self {
        self.examples.insert(name.into(), RefOr::Item(example));
        self
    }
}
