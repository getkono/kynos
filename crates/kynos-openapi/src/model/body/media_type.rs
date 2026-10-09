//! The Media Type Object.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::encoding::Encoding,
        example::{
            Example, Examples, ExamplesConflict, examples_from, examples_into, examples_with_named,
        },
        extensions::Extensions,
        reference::{Ref, RefOr},
        schema::Schema,
    },
};

/// Media types whose payload is a sequence of items rather than one value.
///
/// Introduced in OpenAPI 3.2, alongside [`MediaType::item_schema`] to describe
/// the individual items.
#[cfg(feature = "openapi32")]
pub const SEQUENTIAL_MEDIA_TYPES: &[&str] = &[
    "application/jsonl",
    crate::model::body::mime_names::APPLICATION_NDJSON,
    crate::model::body::mime_names::APPLICATION_JSON_SEQ,
    "application/geo+json-seq",
    crate::model::body::mime_names::TEXT_EVENT_STREAM,
    "multipart/mixed",
    // RFC 9110 section 14.6: a multi-part 206 is a sequence of parts, as 3.2's
    // *Streaming Byte Ranges* example describes with `itemSchema`.
    "multipart/byteranges",
];

/// One representation of a request or response body.
///
/// The examples are held as one [`Examples`], which is the inline `example` or
/// the named `examples` and never both.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawMediaType", into = "RawMediaType")]
pub struct MediaType {
    /// The schema of the complete content.
    ///
    /// For a [sequential media type](SEQUENTIAL_MEDIA_TYPES) this describes the
    /// whole stream as an array; use [`item_schema`](MediaType::item_schema) to
    /// describe items as they arrive.
    pub schema: Option<Schema>,

    /// The schema of each item within a sequential media type.
    ///
    /// Introduced in OpenAPI 3.2. May be used alongside
    /// [`schema`](MediaType::schema).
    #[cfg(feature = "openapi32")]
    pub item_schema: Option<Schema>,

    examples: Option<Examples>,

    /// Encoding information for named properties.
    ///
    /// Applies only to `multipart` and `application/x-www-form-urlencoded`
    /// bodies, and only for keys that exist as properties of the schema. Must
    /// not be combined with [`prefix_encoding`](MediaType::prefix_encoding) or
    /// [`item_encoding`](MediaType::item_encoding).
    pub encoding: Map<Encoding>,

    /// Positional encoding information for the leading array items.
    ///
    /// Introduced in OpenAPI 3.2, for `multipart` bodies with a fixed part
    /// order. Requires an array [`schema`](MediaType::schema) or an
    /// [`item_schema`](MediaType::item_schema).
    #[cfg(feature = "openapi32")]
    pub prefix_encoding: Option<Vec<Encoding>>,

    /// Encoding information applied to every remaining array item.
    ///
    /// Introduced in OpenAPI 3.2. Together with
    /// [`item_schema`](MediaType::item_schema) this describes streaming
    /// `multipart` content.
    #[cfg(feature = "openapi32")]
    pub item_encoding: Option<Box<Encoding>>,

    /// Specification extensions.
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

    /// Shows the body with one inline example.
    ///
    /// Replaces any named examples; the two forms exclude each other.
    #[must_use]
    pub fn with_example(mut self, value: impl Into<Value>) -> Self {
        self.examples = Some(Examples::Inline(value.into()));
        self
    }

    /// Adds a named example, replacing any inline one.
    #[must_use]
    pub fn with_named_example(mut self, name: impl Into<String>, example: Example) -> Self {
        self.examples = Some(examples_with_named(
            self.examples,
            name.into(),
            RefOr::Item(example),
        ));
        self
    }

    /// Adds a named example held in
    /// [`Components::examples`](crate::Components::examples).
    #[must_use]
    pub fn with_named_example_ref(mut self, name: impl Into<String>, example: Ref) -> Self {
        self.examples = Some(examples_with_named(
            self.examples,
            name.into(),
            RefOr::Ref(example),
        ));
        self
    }

    /// The examples this media type carries, if it carries any.
    #[must_use]
    pub fn examples(&self) -> Option<&Examples> {
        self.examples.as_ref()
    }

    /// The inline example, when the body is shown with one.
    #[must_use]
    pub fn example(&self) -> Option<&Value> {
        match &self.examples {
            Some(Examples::Inline(value)) => Some(value),
            Some(Examples::Named(_)) | None => None,
        }
    }

    /// The named examples, when the body is shown with those.
    #[must_use]
    pub fn named_examples(&self) -> Option<&Map<RefOr<Example>>> {
        match &self.examples {
            Some(Examples::Named(named)) => Some(named),
            Some(Examples::Inline(_)) | None => None,
        }
    }
}

/// The wire shape: the example fields flat, as the specification writes them.
#[derive(Serialize, Deserialize)]
struct RawMediaType {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema: Option<Schema>,

    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "itemSchema",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    item_schema: Option<Schema>,

    #[serde(
        default,
        deserialize_with = "crate::model::nullable::some",
        skip_serializing_if = "Option::is_none"
    )]
    example: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    examples: Option<Map<RefOr<Example>>>,

    #[serde(default, skip_serializing_if = "Map::is_empty")]
    encoding: Map<Encoding>,

    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "prefixEncoding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    prefix_encoding: Option<Vec<Encoding>>,

    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "itemEncoding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    item_encoding: Option<Box<Encoding>>,

    #[serde(flatten)]
    extensions: Extensions,
}

impl TryFrom<RawMediaType> for MediaType {
    type Error = ExamplesConflict;

    fn try_from(raw: RawMediaType) -> Result<Self, Self::Error> {
        Ok(Self {
            schema: raw.schema,
            #[cfg(feature = "openapi32")]
            item_schema: raw.item_schema,
            examples: examples_from(raw.example, raw.examples)?,
            encoding: raw.encoding,
            #[cfg(feature = "openapi32")]
            prefix_encoding: raw.prefix_encoding,
            #[cfg(feature = "openapi32")]
            item_encoding: raw.item_encoding,
            extensions: raw.extensions,
        })
    }
}

impl From<MediaType> for RawMediaType {
    fn from(media_type: MediaType) -> Self {
        let (example, examples) = examples_into(media_type.examples);

        Self {
            schema: media_type.schema,
            #[cfg(feature = "openapi32")]
            item_schema: media_type.item_schema,
            example,
            examples,
            encoding: media_type.encoding,
            #[cfg(feature = "openapi32")]
            prefix_encoding: media_type.prefix_encoding,
            #[cfg(feature = "openapi32")]
            item_encoding: media_type.item_encoding,
            extensions: media_type.extensions,
        }
    }
}

#[cfg(test)]
mod tests;
