//! The Header Object, and the headers the specification refuses to describe.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::media_type::MediaType,
        example::Example,
        extensions::Extensions,
        parameter::{ShapeConflict, shape_from, style::Style},
        reference::RefOr,
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

/// A response header, or a header used by an [`Encoding`](crate::Encoding).
///
/// This is a Parameter Object without `name` and `in`. Its style, when present,
/// must be [`Style::Simple`], and `allowEmptyValue` must not be used.
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

    /// A single example of the header value.
    pub example: Option<Value>,

    /// Examples of the header value.
    pub examples: Map<RefOr<Example>>,

    /// Specification extensions.
    pub extensions: Extensions,
}

/// How a header's value is described.
///
/// [`ParameterShape`](crate::model::parameter::ParameterShape) without
/// `allowReserved`: header values are not URI-encoded, so there is no reserved
/// set to allow through, and a field for it would be a question with no answer.
#[derive(Clone, Debug, PartialEq)]
pub enum HeaderShape {
    /// A schema, plus how its value is serialized.
    Schema {
        /// The schema defining the header's type.
        schema: Schema,

        /// How the value is serialized. Must be [`Style::Simple`] when present.
        style: Option<Style>,

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
            example: None,
            examples: Map::new(),
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
    pub fn style(&self) -> Option<Style> {
        match self.shape {
            HeaderShape::Schema { style, .. } => style,
            HeaderShape::Content { .. } => None,
        }
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
    style: Option<Style>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    explode: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema: Option<Schema>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    example: Option<Value>,

    #[serde(default, skip_serializing_if = "Map::is_empty")]
    examples: Map<RefOr<Example>>,

    #[serde(default, skip_serializing_if = "Map::is_empty")]
    content: Map<MediaType>,

    #[serde(flatten)]
    extensions: Extensions,
}

impl TryFrom<RawHeader> for Header {
    type Error = ShapeConflict;

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
            example: raw.example,
            examples: raw.examples,
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

        Self {
            description: header.description,
            required: header.required,
            deprecated: header.deprecated,
            style,
            explode,
            schema,
            example: header.example,
            examples: header.examples,
            content,
            extensions: header.extensions,
        }
    }
}
