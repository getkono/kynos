//! The Parameter, Header and Style Objects.

pub mod header;
pub mod style;
mod wire;

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::media_type::MediaType,
        example::{Example, Examples, examples_with_named},
        extensions::Extensions,
        parameter::style::Style,
        reference::{Ref, RefOr},
        schema::Schema,
    },
};

/// Where a parameter is carried.
///
/// `#[non_exhaustive]` because `openapi32` adds a variant and Cargo unifies
/// features across a build.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParameterIn {
    /// A named query string parameter.
    ///
    /// The default only nominally; the constructors always set a location.
    #[default]
    Query,
    /// A request header.
    ///
    /// `Accept`, `Content-Type` and `Authorization` must **not** be declared
    /// this way; the specification ignores such a definition. Use the `content`
    /// map and a [`SecurityScheme`](crate::SecurityScheme) instead.
    Header,
    /// A variable in the path template. Always required.
    Path,
    /// A cookie.
    Cookie,
    /// The entire query string, described by media type.
    ///
    /// Introduced in OpenAPI 3.2, for query strings named parameters cannot
    /// express — nested filters, JSON, RFC 9535 JSONPath. It must be the only
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
#[serde(try_from = "wire::RawParameter", into = "wire::RawParameter")]
pub struct Parameter {
    /// The name of the parameter, case-sensitive.
    ///
    /// For [`ParameterIn::Path`] this must correspond to exactly one template
    /// expression in the path.
    pub name: String,

    /// Where the parameter is carried.
    pub location: ParameterIn,

    /// A description of the parameter. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    pub description: Option<String>,

    /// Whether the parameter is mandatory.
    ///
    /// Must be `true` when [`location`](Parameter::location) is
    /// [`ParameterIn::Path`].
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
/// `style`, `explode` and `allowReserved` only apply alongside a schema, so they
/// live in that variant.
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
    /// The specification allows exactly one entry, so this holds one pair.
    Content {
        /// The media type the value is carried as.
        media_type: String,

        /// What that media type describes, inline or by reference.
        ///
        /// A reference is 3.2's alone: 3.1 admits only a Media Type Object here.
        value: Box<RefOr<MediaType>>,
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
        value: impl Into<RefOr<MediaType>>,
    ) -> Self {
        Self::shaped(
            name,
            location,
            ParameterShape::Content {
                media_type: media_type.into(),
                value: Box::new(value.into()),
            },
        )
    }

    fn shaped(name: impl Into<String>, location: ParameterIn, shape: ParameterShape) -> Self {
        Self {
            name: name.into(),
            location,
            description: None,
            // A path parameter is required by definition.
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

    /// How this parameter's value is described.
    #[must_use]
    pub fn shape(&self) -> &ParameterShape {
        &self.shape
    }

    /// The same, mutably; every [`ParameterShape`] is a valid description.
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

    /// The media type and its description, inline or by reference, when this
    /// parameter uses `content`.
    #[must_use]
    pub fn content(&self) -> Option<(&str, &RefOr<MediaType>)> {
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
        self.examples.as_ref().and_then(Examples::as_inline)
    }

    /// The named examples, when the value is shown with those.
    #[must_use]
    pub fn named_examples(&self) -> Option<&Map<RefOr<Example>>> {
        self.examples.as_ref().and_then(Examples::as_named)
    }

    /// Returns the effective style, falling back to the location's default.
    ///
    /// `None` for a content-described parameter, to which `style` does not apply.
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

#[cfg(test)]
mod tests;
