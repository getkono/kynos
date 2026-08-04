//! The Request Body, Media Type and Encoding Objects.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    example::Example,
    extensions::Extensions,
    parameter::{Header, Style},
    reference::RefOr,
    schema::Schema,
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

/// A request body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestBody {
    /// A description of the body. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The body's representations, keyed by media type or media type range.
    ///
    /// More than one entry describes a body the server accepts in several
    /// encodings.
    pub content: Map<MediaType>,

    /// Whether the body is mandatory. Defaults to `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl RequestBody {
    /// Creates a required body with a single media type.
    pub fn new(media_type: impl Into<String>, content: MediaType) -> Self {
        let mut map = Map::new();
        map.insert(media_type.into(), content);
        Self {
            content: map,
            required: Some(true),
            ..Self::default()
        }
    }

    /// Creates a required `application/json` body.
    #[must_use]
    pub fn json(schema: Schema) -> Self {
        Self::new(mime_names::APPLICATION_JSON, MediaType::new(schema))
    }

    /// Adds another representation of the same body.
    #[must_use]
    pub fn with_media_type(mut self, media_type: impl Into<String>, content: MediaType) -> Self {
        self.content.insert(media_type.into(), content);
        self
    }

    /// Marks the body optional.
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.required = Some(false);
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

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

/// How a single property of a `multipart` or form-urlencoded body is encoded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Encoding {
    /// The media type of the property, or a comma-separated list of them.
    #[serde(
        rename = "contentType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub content_type: Option<String>,

    /// Headers accompanying this part. `multipart` only.
    ///
    /// A `Content-Type` entry here is ignored, since
    /// [`content_type`](Encoding::content_type) states it.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub headers: Map<RefOr<Header>>,

    /// How the property value is serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,

    /// Whether an array or object generates one entry per member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explode: Option<bool>,

    /// Whether reserved URI characters may appear unencoded.
    #[serde(
        rename = "allowReserved",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub allow_reserved: Option<bool>,

    /// Encoding for the properties of a nested `multipart` part.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub encoding: Map<Encoding>,

    /// Positional encoding for the leading items of a nested part.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "prefixEncoding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub prefix_encoding: Option<Vec<Encoding>>,

    /// Encoding for every remaining item of a nested part.
    ///
    /// Introduced in OpenAPI 3.2.
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

impl Encoding {
    /// Encodes a property with the given media type.
    pub fn new(content_type: impl Into<String>) -> Self {
        Self {
            content_type: Some(content_type.into()),
            ..Self::default()
        }
    }

    /// Declares a header accompanying this part.
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, header: Header) -> Self {
        self.headers.insert(name.into(), RefOr::Item(header));
        self
    }
}

/// Media type names used often enough to be worth naming.
///
/// These are plain string constants rather than `mime::Mime` values because the
/// document model must be able to carry media type *ranges* and vendor types
/// that a parsed `Mime` would normalize.
pub mod mime_names {
    /// `application/json`.
    pub const APPLICATION_JSON: &str = "application/json";
    /// `application/problem+json`, the RFC 9457 error format.
    pub const APPLICATION_PROBLEM_JSON: &str = "application/problem+json";
    /// `application/x-www-form-urlencoded`.
    pub const APPLICATION_FORM_URLENCODED: &str = "application/x-www-form-urlencoded";
    /// `application/octet-stream`.
    pub const APPLICATION_OCTET_STREAM: &str = "application/octet-stream";
    /// `multipart/form-data`.
    pub const MULTIPART_FORM_DATA: &str = "multipart/form-data";
    /// `text/plain`.
    pub const TEXT_PLAIN: &str = "text/plain";
    /// `text/event-stream`, the Server-Sent Events format.
    pub const TEXT_EVENT_STREAM: &str = "text/event-stream";
    /// `application/x-ndjson`, newline-delimited JSON.
    pub const APPLICATION_NDJSON: &str = "application/x-ndjson";
    /// `application/json-seq`, RFC 7464 JSON text sequences.
    pub const APPLICATION_JSON_SEQ: &str = "application/json-seq";
}

#[cfg(test)]
mod tests {
    use super::{MediaType, RequestBody, mime_names};
    use crate::schema::{Schema, SchemaType};

    #[test]
    fn a_json_body_is_required_by_default() {
        let body = RequestBody::json(Schema::of_type(SchemaType::Object));
        assert_eq!(body.required, Some(true));
        assert!(body.content.contains_key(mime_names::APPLICATION_JSON));
    }

    #[test]
    fn a_body_can_offer_several_representations() {
        let body = RequestBody::json(Schema::of_type(SchemaType::Object)).with_media_type(
            mime_names::APPLICATION_FORM_URLENCODED,
            MediaType::new(Schema::of_type(SchemaType::Object)),
        );
        assert_eq!(body.content.len(), 2);
    }

    #[test]
    fn optional_bodies_say_so() {
        let body = RequestBody::json(Schema::any()).optional();
        assert_eq!(body.required, Some(false));
    }
}
