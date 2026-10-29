//! The flat wire shapes of the Parameter and Header Objects, and the
//! conversions that fold them into the typed shapes or refuse them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::media_type::MediaType,
        example::{Example, ExamplesConflict, examples_from, examples_into},
        extensions::Extensions,
        parameter::{
            Parameter, ParameterIn, ParameterShape,
            header::{Header, HeaderShape},
            style::{HeaderStyle, Style},
        },
        reference::RefOr,
        schema::Schema,
    },
};

/// The wire shape: the value fields flat, as the specification writes them.
#[derive(Serialize, Deserialize)]
pub(super) struct RawParameter {
    name: String,

    #[serde(rename = "in")]
    location: ParameterIn,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    required: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    deprecated: Option<bool>,

    #[serde(
        rename = "allowEmptyValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    allow_empty_value: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    style: Option<Style>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    explode: Option<bool>,

    #[serde(
        rename = "allowReserved",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    allow_reserved: Option<bool>,

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
    content: Map<RefOr<MediaType>>,

    #[serde(flatten)]
    extensions: Extensions,
}

/// The wire shape: the value fields flat, as the specification writes them.
#[derive(Serialize, Deserialize)]
pub(super) struct RawHeader {
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
    content: Map<RefOr<MediaType>>,

    #[serde(flatten)]
    extensions: Extensions,
}

/// A Parameter or Header Object whose fields do not hold together.
#[derive(Debug, thiserror::Error)]
pub(super) enum ParameterConflict {
    /// The value is described by neither `schema` nor `content`, or by both.
    #[error(transparent)]
    Shape(#[from] ShapeConflict),

    /// The value is shown by both `example` and `examples`.
    #[error(transparent)]
    Examples(#[from] ExamplesConflict),
}

/// A Parameter or Header Object whose value description does not hold together.
#[derive(Debug, thiserror::Error)]
pub(super) enum ShapeConflict {
    /// Neither `schema` nor `content` was given.
    #[error("one of `schema` and `content` is required")]
    Neither,

    /// Both `schema` and `content` were given.
    #[error("`schema` and `content` are mutually exclusive")]
    Both,

    /// `content` held a number of entries other than one.
    #[error("`content` must hold exactly one entry, found {0}")]
    ContentNotSingular(usize),
}

/// The one `content` entry a shape may hold, keyed by its media type.
type ContentEntry = (String, RefOr<MediaType>);

/// Splits the wire fields into a shape, or says why they will not go.
fn shape_from(
    schema: Option<Schema>,
    content: Map<RefOr<MediaType>>,
) -> Result<(Schema, Option<ContentEntry>), ShapeConflict> {
    match (schema, content.len()) {
        (Some(_), 1..) => Err(ShapeConflict::Both),
        (None, 0) => Err(ShapeConflict::Neither),
        (None, 1) => {
            let (media_type, value) = content
                .into_iter()
                .next()
                .expect("a map of length one has an entry");
            Ok((Schema::Bool(true), Some((media_type, value))))
        }
        (None, found) => Err(ShapeConflict::ContentNotSingular(found)),
        (Some(schema), 0) => Ok((schema, None)),
    }
}

impl TryFrom<RawParameter> for Parameter {
    type Error = ParameterConflict;

    fn try_from(raw: RawParameter) -> Result<Self, Self::Error> {
        let shape = match shape_from(raw.schema, raw.content)? {
            (_, Some((media_type, value))) => ParameterShape::Content {
                media_type,
                value: Box::new(value),
            },
            (schema, None) => ParameterShape::Schema {
                schema,
                style: raw.style,
                explode: raw.explode,
                allow_reserved: raw.allow_reserved,
            },
        };

        Ok(Self {
            name: raw.name,
            location: raw.location,
            description: raw.description,
            required: raw.required,
            deprecated: raw.deprecated,
            allow_empty_value: raw.allow_empty_value,
            shape,
            examples: examples_from(raw.example, raw.examples)?,
            extensions: raw.extensions,
        })
    }
}

impl From<Parameter> for RawParameter {
    fn from(parameter: Parameter) -> Self {
        let (schema, style, explode, allow_reserved, content) = match parameter.shape {
            ParameterShape::Schema {
                schema,
                style,
                explode,
                allow_reserved,
            } => (Some(schema), style, explode, allow_reserved, Map::new()),
            ParameterShape::Content { media_type, value } => (
                None,
                None,
                None,
                None,
                Map::from_iter([(media_type, *value)]),
            ),
        };

        let (example, examples) = examples_into(parameter.examples);

        Self {
            name: parameter.name,
            location: parameter.location,
            description: parameter.description,
            required: parameter.required,
            deprecated: parameter.deprecated,
            allow_empty_value: parameter.allow_empty_value,
            style,
            explode,
            allow_reserved,
            schema,
            example,
            examples,
            content,
            extensions: parameter.extensions,
        }
    }
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
