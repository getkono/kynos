//! The Header Object, and the headers the specification refuses to describe.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::media_type::MediaType,
        example::{Example, Examples, examples_from, examples_into, examples_with_named},
        extensions::Extensions,
        parameter::{ParameterConflict, shape_from, style::HeaderStyle},
        reference::{Ref, RefOr},
        schema::Schema,
    },
};

/// Header names that must not be declared as
/// [`ParameterIn::Header`](crate::model::parameter::ParameterIn::Header)
/// parameters.
///
/// The specification states that a parameter definition for any of these shall
/// be ignored, which makes declaring one a silent lie in the description.
pub const IGNORED_HEADER_PARAMETERS: &[&str] = &["Accept", "Content-Type", "Authorization"];

/// Whether `name` is a header that must not be declared as a parameter.
///
/// Comparison is ASCII case-insensitive, matching HTTP header semantics.
#[must_use]
pub fn is_ignored_header_parameter(name: &str) -> bool {
    IGNORED_HEADER_PARAMETERS
        .iter()
        .any(|ignored| ignored.eq_ignore_ascii_case(name))
}

/// Header names that must not be declared in a `headers` map.
///
/// A response states its media type in `content` and an encoded part states
/// its own in `contentType`, so the specification says a `Content-Type` entry
/// in either map shall be ignored. The list is shorter than
/// [`IGNORED_HEADER_PARAMETERS`] because `Accept` and `Authorization` are
/// request headers, which neither map describes.
pub const IGNORED_HEADERS: &[&str] = &["Content-Type"];

/// Whether `name` is a header that must not be declared in a `headers` map.
///
/// Comparison is ASCII case-insensitive, as it is for a parameter.
#[must_use]
pub fn is_ignored_header(name: &str) -> bool {
    IGNORED_HEADERS
        .iter()
        .any(|ignored| ignored.eq_ignore_ascii_case(name))
}

/// A response header, or a header used by an [`Encoding`](crate::Encoding).
///
/// This is a Parameter Object without `name` and `in`, and without
/// `allowEmptyValue`, which the specification refuses a header. Like a
/// parameter it holds one [`HeaderShape`] and one [`Examples`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawHeader", into = "RawHeader")]
pub struct Header {
    /// A description of the header. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    pub description: Option<String>,

    /// Whether the header is mandatory.
    pub required: Option<bool>,

    /// Whether the header is deprecated.
    pub deprecated: Option<bool>,

    shape: HeaderShape,

    examples: Option<Examples>,

    /// Specification extensions.
    pub extensions: Extensions,
}

/// How a header's value is described.
///
/// [`ParameterShape`](crate::model::parameter::ParameterShape) without
/// `allowReserved`: header values are not URI-encoded, so there is no reserved
/// set to allow through, and a field for it would be a question with no answer.
///
/// That is 3.1's reasoning, and 3.1 agrees — it forbids `allowReserved` on a
/// Header Object outright. **3.2 does not.** It drops the field from that
/// prohibition, so a 3.2 Header Object may carry one and this type cannot hold
/// it: such a header loses the field on a round trip. That is a missing 3.2
/// feature rather than a wrong answer to 3.1's question, and it is deliberately
/// not fixed here — adding it widens the model rather than correcting it.
#[derive(Clone, Debug, PartialEq)]
pub enum HeaderShape {
    /// A schema, plus how its value is serialized.
    Schema {
        /// The schema defining the header's type.
        schema: Schema,

        /// How the value is serialized.
        style: Option<HeaderStyle>,

        /// Whether an array or object generates one value per member.
        explode: Option<bool>,
    },

    /// One media type describing the value.
    ///
    /// Boxed for the reason
    /// [`ParameterShape::Content`](crate::model::parameter::ParameterShape::Content)
    /// is: a `MediaType` dwarfs the schema-side fields beside it.
    Content {
        /// The media type the value is carried as.
        media_type: String,

        /// What that media type describes.
        value: Box<MediaType>,
    },
}

impl Header {
    /// Creates a header described by a schema.
    pub fn new(schema: Schema) -> Self {
        Self::shaped(HeaderShape::Schema {
            schema,
            style: None,
            explode: None,
        })
    }

    /// Creates a header described by one media type.
    pub fn with_content(media_type: impl Into<String>, value: MediaType) -> Self {
        Self::shaped(HeaderShape::Content {
            media_type: media_type.into(),
            value: Box::new(value),
        })
    }

    fn shaped(shape: HeaderShape) -> Self {
        Self {
            description: None,
            required: None,
            deprecated: None,
            shape,
            examples: None,
            extensions: Extensions::default(),
        }
    }

    /// How this header's value is described.
    #[must_use]
    pub fn shape(&self) -> &HeaderShape {
        &self.shape
    }

    /// The same, mutably.
    ///
    /// Handing out `&mut` costs nothing here: every [`HeaderShape`] is a valid
    /// description, so there is no combination a caller could reach by editing
    /// one that it could not reach by building one.
    pub fn shape_mut(&mut self) -> &mut HeaderShape {
        &mut self.shape
    }

    /// The schema, when this header is described by one.
    #[must_use]
    pub fn schema(&self) -> Option<&Schema> {
        match &self.shape {
            HeaderShape::Schema { schema, .. } => Some(schema),
            HeaderShape::Content { .. } => None,
        }
    }

    /// The media type and its description, when this header uses `content`.
    #[must_use]
    pub fn content(&self) -> Option<(&str, &MediaType)> {
        match &self.shape {
            HeaderShape::Content { media_type, value } => Some((media_type, &**value)),
            HeaderShape::Schema { .. } => None,
        }
    }

    /// The declared style, if any.
    #[must_use]
    pub fn style(&self) -> Option<HeaderStyle> {
        match self.shape {
            HeaderShape::Schema { style, .. } => style,
            HeaderShape::Content { .. } => None,
        }
    }

    /// Sets the serialization style and explode flag.
    ///
    /// A no-op on a content-described header, which has no style to set.
    ///
    /// Naming [`HeaderStyle::Simple`] is not redundant even though it is the
    /// only style a header may take: the specification distinguishes a header
    /// that states it from one that leaves it out, and a description that
    /// stated it is emitted back the way it arrived.
    #[must_use]
    pub fn with_style(mut self, style: HeaderStyle, explode: bool) -> Self {
        if let HeaderShape::Schema {
            style: slot,
            explode: exploded,
            ..
        } = &mut self.shape
        {
            *slot = Some(style);
            *exploded = Some(explode);
        }
        self
    }

    /// The examples this header carries, if it carries any.
    #[must_use]
    pub fn examples(&self) -> Option<&Examples> {
        self.examples.as_ref()
    }

    /// The inline example, when the value is shown with one.
    #[must_use]
    pub fn example(&self) -> Option<&Value> {
        match &self.examples {
            Some(Examples::Inline(value)) => Some(value),
            Some(Examples::Named(_)) | None => None,
        }
    }

    /// The named examples, when the value is shown with those.
    #[must_use]
    pub fn named_examples(&self) -> Option<&Map<RefOr<Example>>> {
        match &self.examples {
            Some(Examples::Named(named)) => Some(named),
            Some(Examples::Inline(_)) | None => None,
        }
    }

    /// Shows the value with one inline example.
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

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Marks the header mandatory.
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.required = Some(required);
        self
    }
}

/// The wire shape: the value fields flat, as the specification writes them.
#[derive(Serialize, Deserialize)]
struct RawHeader {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    required: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    deprecated: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    style: Option<HeaderStyle>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    explode: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema: Option<Schema>,

    #[serde(
        default,
        deserialize_with = "crate::model::nullable::some",
        skip_serializing_if = "Option::is_none"
    )]
    example: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    examples: Option<Map<RefOr<Example>>>,

    #[serde(default, skip_serializing_if = "Map::is_empty")]
    content: Map<MediaType>,

    #[serde(flatten)]
    extensions: Extensions,
}

impl TryFrom<RawHeader> for Header {
    type Error = ParameterConflict;

    fn try_from(raw: RawHeader) -> Result<Self, Self::Error> {
        let shape = match shape_from(raw.schema, raw.content)? {
            (_, Some((media_type, value))) => HeaderShape::Content {
                media_type,
                value: Box::new(value),
            },
            (schema, None) => HeaderShape::Schema {
                schema,
                style: raw.style,
                explode: raw.explode,
            },
        };

        Ok(Self {
            description: raw.description,
            required: raw.required,
            deprecated: raw.deprecated,
            shape,
            examples: examples_from(raw.example, raw.examples)?,
            extensions: raw.extensions,
        })
    }
}

impl From<Header> for RawHeader {
    fn from(header: Header) -> Self {
        let (schema, style, explode, content) = match header.shape {
            HeaderShape::Schema {
                schema,
                style,
                explode,
            } => (Some(schema), style, explode, Map::new()),
            HeaderShape::Content { media_type, value } => {
                (None, None, None, Map::from_iter([(media_type, *value)]))
            }
        };

        let (example, examples) = examples_into(header.examples);

        Self {
            description: header.description,
            required: header.required,
            deprecated: header.deprecated,
            style,
            explode,
            schema,
            example,
            examples,
            content,
            extensions: header.extensions,
        }
    }
}
