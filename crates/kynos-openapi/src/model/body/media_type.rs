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
    // RFC 9110 section 14.6. A 206 carrying several parts holds an unnamed,
    // request-determined number of them, each with its own `Content-Range` --
    // which is a sequence rather than a value, and which 3.2's own
    // *Streaming Byte Ranges* example describes with `itemSchema` and
    // `itemEncoding`.
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
    /// whole stream treated as an array, which is only useful to a consumer
    /// willing to buffer it. Use [`item_schema`](MediaType::item_schema) to
    /// describe items that are processed as they arrive.
    pub schema: Option<Schema>,

    /// The schema of each item within a sequential media type.
    ///
    /// Introduced in OpenAPI 3.2. This is what makes Server-Sent Events, JSON
    /// Lines and JSON Text Sequences describable at all; it may be used
    /// alongside [`schema`](MediaType::schema).
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
    /// Replaces any named examples: the two forms exclude each other, so there
    /// is no state that holds both.
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

    #[serde(default, skip_serializing_if = "Option::is_none")]
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
mod tests {
    use serde_json::json;

    use super::{Example, Examples, MediaType};

    /// The whole table, transcribed.
    ///
    /// A closed enumeration under `docs/testing.md`: the list is the claim, so
    /// a row added or removed without a reader noticing fails here rather than
    /// being sampled around. Every entry is a media type OpenAPI 3.2 gives
    /// `itemSchema` for, and there is no membership rule to derive one from —
    /// the specification names them.
    #[cfg(feature = "openapi32")]
    #[test]
    fn the_sequential_media_type_table_is_closed() {
        assert_eq!(
            super::SEQUENTIAL_MEDIA_TYPES,
            [
                "application/jsonl",
                "application/x-ndjson",
                "application/json-seq",
                "application/geo+json-seq",
                "text/event-stream",
                "multipart/mixed",
                "multipart/byteranges",
            ]
        );
    }

    /// No entry is listed twice, and each is a media type.
    #[cfg(feature = "openapi32")]
    #[test]
    fn every_row_is_a_media_type_named_once() {
        let mut seen: Vec<&str> = super::SEQUENTIAL_MEDIA_TYPES.to_vec();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();

        assert_eq!(seen.len(), before, "a media type is listed more than once");
        for media_type in super::SEQUENTIAL_MEDIA_TYPES {
            assert!(
                media_type.contains('/') && *media_type == media_type.to_ascii_lowercase(),
                "`{media_type}` is not a media type"
            );
        }
    }

    #[test]
    fn a_body_shown_both_ways_at_once_is_refused() {
        let error =
            serde_json::from_str::<MediaType>(r#"{"example":1,"examples":{"one":{"value":1}}}"#)
                .expect_err("`example` is exclusive with `examples`");

        assert!(error.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn each_form_of_example_round_trips() {
        for media_type in [
            MediaType::default().with_example(json!({"id": 1})),
            MediaType::default().with_named_example("one", Example::new(1)),
        ] {
            let json = serde_json::to_string(&media_type).expect("serializable");
            let parsed: MediaType = serde_json::from_str(&json).expect("deserializable");
            assert_eq!(parsed, media_type);
        }
    }

    #[test]
    fn a_named_example_replaces_an_inline_one() {
        let media_type = MediaType::default()
            .with_example(json!("inline"))
            .with_named_example("one", Example::new(1));

        assert!(media_type.example().is_none());
        assert!(matches!(media_type.examples(), Some(Examples::Named(named)) if named.len() == 1));
    }

    #[test]
    fn an_inline_example_replaces_named_ones() {
        let media_type = MediaType::default()
            .with_named_example("one", Example::new(1))
            .with_example(json!("inline"));

        assert!(media_type.named_examples().is_none());
        assert_eq!(media_type.example(), Some(&json!("inline")));
    }
}
