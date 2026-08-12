//! The Parameter, Header and Style Objects.

pub mod header;
pub mod style;

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::media_type::MediaType,
        example::{
            Example, Examples, ExamplesConflict, examples_from, examples_into, examples_with_named,
        },
        extensions::Extensions,
        parameter::style::Style,
        reference::{Ref, RefOr},
        schema::Schema,
    },
};

/// Where a parameter is carried.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParameterIn {
    /// A named query string parameter.
    ///
    /// The default only because a location has to be one of these; a parameter
    /// built through the constructors always has its location set explicitly.
    #[default]
    Query,
    /// A request header.
    ///
    /// Note that `Accept`, `Content-Type` and `Authorization` must **not** be
    /// declared this way: the specification says such a definition shall be
    /// ignored. Content negotiation belongs in the `content` map, and
    /// credentials belong in a [`SecurityScheme`](crate::SecurityScheme).
    Header,
    /// A variable in the path template. Always required.
    Path,
    /// A cookie.
    Cookie,
    /// The entire query string, described by media type.
    ///
    /// Introduced in OpenAPI 3.2. This is the sanctioned way to describe query
    /// strings that a sequence of named parameters cannot express — nested
    /// filters, JSON in the query, RFC 9535 JSONPath. It must be the only
    /// query-related parameter on its operation.
    #[cfg(feature = "openapi32")]
    Querystring,
}

impl ParameterIn {
    /// The location as it is spelled in a description.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Header => "header",
            Self::Path => "path",
            Self::Cookie => "cookie",
            #[cfg(feature = "openapi32")]
            Self::Querystring => "querystring",
        }
    }
}

impl fmt::Display for ParameterIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single operation parameter.
///
/// The value is described by a [`ParameterShape`], which is one of `schema` or
/// `content` and never both or neither, and shown by an [`Examples`], which is
/// the inline `example` or the named `examples` and never both.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawParameter", into = "RawParameter")]
pub struct Parameter {
    /// The name of the parameter, case-sensitive.
    ///
    /// For [`ParameterIn::Path`] this must correspond to exactly one template
    /// expression in the path.
    pub name: String,

    /// Where the parameter is carried.
    #[serde(rename = "in")]
    pub location: ParameterIn,

    /// A description of the parameter. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether the parameter is mandatory.
    ///
    /// Must be `true` when [`location`](Parameter::location) is
    /// [`ParameterIn::Path`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// Whether the parameter is deprecated.
    pub deprecated: Option<bool>,

    /// Whether an empty value is permitted. Query parameters only.
    ///
    /// Not recommended in 3.1, and formally deprecated in 3.2.
    pub allow_empty_value: Option<bool>,

    shape: ParameterShape,

    examples: Option<Examples>,

    /// Specification extensions.
    pub extensions: Extensions,
}

/// How a parameter's value is described.
///
/// A parameter carries `schema` or `content`, never both and never neither.
/// `style`, `explode` and `allowReserved` only mean anything alongside a
/// schema, so they live in that variant rather than beside it — setting a style
/// on a content-described parameter is not a mistake to report, it is a
/// sentence with nowhere to be written.
#[derive(Clone, Debug, PartialEq)]
pub enum ParameterShape {
    /// The simple case: a schema, plus how its value is serialized.
    Schema {
        /// The schema defining the parameter's type.
        schema: Schema,

        /// How the value is serialized.
        style: Option<Style>,

        /// Whether an array or object generates one parameter per member.
        explode: Option<bool>,

        /// Whether reserved URI characters may appear unencoded.
        allow_reserved: Option<bool>,
    },

    /// The complex case: one media type describing the value.
    ///
    /// The specification allows exactly one entry, so this holds one pair
    /// rather than a map that has to be counted. Boxed because a `MediaType`
    /// dwarfs the schema-side fields, and every parameter would otherwise pay
    /// for the larger of the two.
    Content {
        /// The media type the value is carried as.
        media_type: String,

        /// What that media type describes.
        value: Box<MediaType>,
    },
}

impl Parameter {
    /// Creates a schema-described parameter.
    pub fn new(name: impl Into<String>, location: ParameterIn, schema: Schema) -> Self {
        Self::shaped(
            name,
            location,
            ParameterShape::Schema {
                schema,
                style: None,
                explode: None,
                allow_reserved: None,
            },
        )
    }

    /// Creates a parameter described by one media type.
    ///
    /// For the values a sequence of `style` rules cannot express — JSON in a
    /// query string, a nested filter.
    pub fn with_content(
        name: impl Into<String>,
        location: ParameterIn,
        media_type: impl Into<String>,
        value: MediaType,
    ) -> Self {
        Self::shaped(
            name,
            location,
            ParameterShape::Content {
                media_type: media_type.into(),
                value: Box::new(value),
            },
        )
    }

    fn shaped(name: impl Into<String>, location: ParameterIn, shape: ParameterShape) -> Self {
        Self {
            name: name.into(),
            location,
            description: None,
            // A path parameter is required by definition, so filling this in is
            // a correctness measure rather than a convenience.
            required: (location == ParameterIn::Path).then_some(true),
            deprecated: None,
            allow_empty_value: None,
            shape,
            examples: None,
            extensions: Extensions::default(),
        }
    }

    /// Creates a required path parameter.
    pub fn path(name: impl Into<String>, schema: Schema) -> Self {
        Self::new(name, ParameterIn::Path, schema)
    }

    /// Creates a query parameter.
    pub fn query(name: impl Into<String>, schema: Schema) -> Self {
        Self::new(name, ParameterIn::Query, schema)
    }

    /// Creates a header parameter.
    pub fn header(name: impl Into<String>, schema: Schema) -> Self {
        Self::new(name, ParameterIn::Header, schema)
    }

    /// Creates a cookie parameter.
    pub fn cookie(name: impl Into<String>, schema: Schema) -> Self {
        Self::new(name, ParameterIn::Cookie, schema)
    }

    /// Marks the parameter mandatory.
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.required = Some(required);
        self
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the serialization style and explode flag.
    ///
    /// A no-op on a content-described parameter, which has no style to set.
    #[must_use]
    pub fn with_style(mut self, style: Style, explode: bool) -> Self {
        if let ParameterShape::Schema {
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

    /// How this parameter's value is described.
    #[must_use]
    pub fn shape(&self) -> &ParameterShape {
        &self.shape
    }

    /// The same, mutably.
    ///
    /// Handing out `&mut` costs nothing here: every [`ParameterShape`] is a
    /// valid description, so there is no combination a caller could reach by
    /// editing one that it could not reach by building one.
    pub fn shape_mut(&mut self) -> &mut ParameterShape {
        &mut self.shape
    }

    /// The schema, when this parameter is described by one.
    #[must_use]
    pub fn schema(&self) -> Option<&Schema> {
        match &self.shape {
            ParameterShape::Schema { schema, .. } => Some(schema),
            ParameterShape::Content { .. } => None,
        }
    }

    /// The media type and its description, when this parameter uses `content`.
    #[must_use]
    pub fn content(&self) -> Option<(&str, &MediaType)> {
        match &self.shape {
            ParameterShape::Content { media_type, value } => Some((media_type, &**value)),
            ParameterShape::Schema { .. } => None,
        }
    }

    /// The declared style, if any. Always `None` for a content-described
    /// parameter.
    #[must_use]
    pub fn style(&self) -> Option<Style> {
        match self.shape {
            ParameterShape::Schema { style, .. } => style,
            ParameterShape::Content { .. } => None,
        }
    }

    /// Whether reserved URI characters may appear unencoded.
    #[must_use]
    pub fn allow_reserved(&self) -> Option<bool> {
        match self.shape {
            ParameterShape::Schema { allow_reserved, .. } => allow_reserved,
            ParameterShape::Content { .. } => None,
        }
    }

    /// The examples this parameter carries, if it carries any.
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

    /// Returns the effective style, falling back to the location's default.
    ///
    /// `None` for a content-described parameter: `style` does not apply to one,
    /// so there is no default to fall back to either.
    #[must_use]
    pub fn effective_style(&self) -> Option<Style> {
        match self.shape {
            ParameterShape::Schema { style, .. } => {
                Some(style.unwrap_or_else(|| Style::default_for(self.location)))
            }
            ParameterShape::Content { .. } => None,
        }
    }

    /// Returns the effective explode flag, falling back to the style's default.
    #[must_use]
    pub fn effective_explode(&self) -> Option<bool> {
        match self.shape {
            ParameterShape::Schema { explode, .. } => Some(
                explode
                    .unwrap_or_else(|| self.effective_style().is_some_and(Style::default_explode)),
            ),
            ParameterShape::Content { .. } => None,
        }
    }
}

/// The wire shape: the value fields flat, as the specification writes them.
#[derive(Serialize, Deserialize)]
struct RawParameter {
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

    #[serde(default, skip_serializing_if = "Option::is_none")]
    example: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    examples: Option<Map<RefOr<Example>>>,

    #[serde(default, skip_serializing_if = "Map::is_empty")]
    content: Map<MediaType>,

    #[serde(flatten)]
    extensions: Extensions,
}

/// A Parameter or Header Object whose fields do not hold together.
///
/// Two independent ways to be ill-formed, kept apart so that each reads as the
/// sentence the specification writes.
#[derive(Debug)]
pub(crate) enum ParameterConflict {
    /// The value is described by neither `schema` nor `content`, or by both.
    Shape(ShapeConflict),

    /// The value is shown by both `example` and `examples`.
    Examples(ExamplesConflict),
}

impl fmt::Display for ParameterConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shape(conflict) => conflict.fmt(f),
            Self::Examples(conflict) => conflict.fmt(f),
        }
    }
}

impl From<ShapeConflict> for ParameterConflict {
    fn from(conflict: ShapeConflict) -> Self {
        Self::Shape(conflict)
    }
}

impl From<ExamplesConflict> for ParameterConflict {
    fn from(conflict: ExamplesConflict) -> Self {
        Self::Examples(conflict)
    }
}

/// A Parameter or Header Object whose value description does not hold together.
#[derive(Debug)]
pub(crate) enum ShapeConflict {
    /// Neither `schema` nor `content` was given.
    Neither,

    /// Both `schema` and `content` were given.
    Both,

    /// `content` held a number of entries other than one.
    ContentNotSingular(usize),
}

impl fmt::Display for ShapeConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Neither => f.write_str("one of `schema` and `content` is required"),
            Self::Both => f.write_str("`schema` and `content` are mutually exclusive"),
            Self::ContentNotSingular(found) => {
                write!(f, "`content` must hold exactly one entry, found {found}")
            }
        }
    }
}

/// Splits the wire fields into a shape, or says why they will not go.
pub(crate) fn shape_from(
    schema: Option<Schema>,
    content: Map<MediaType>,
) -> Result<(Schema, Option<(String, MediaType)>), ShapeConflict> {
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

#[cfg(test)]
mod tests;
