//! The Parameter, Header and Style Objects.

pub mod header;
pub mod style;

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::media_type::MediaType, example::Example, extensions::Extensions,
        parameter::style::Style, reference::RefOr, schema::Schema,
    },
};

/// Where a parameter is carried.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParameterIn {
    /// A named query string parameter.
    ///
    /// The default, so that [`Parameter`] can derive [`Default`]; a parameter
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
/// Exactly one of [`schema`](Parameter::schema) and
/// [`content`](Parameter::content) must be present.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,

    /// Whether an empty value is permitted. Query parameters only.
    ///
    /// Not recommended in 3.1, and formally deprecated in 3.2.
    #[serde(
        rename = "allowEmptyValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub allow_empty_value: Option<bool>,

    /// How the parameter value is serialized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,

    /// Whether an array or object generates one parameter per member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explode: Option<bool>,

    /// Whether reserved URI characters may appear unencoded.
    #[serde(
        rename = "allowReserved",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub allow_reserved: Option<bool>,

    /// The schema of the parameter value.
    ///
    /// Mutually exclusive with [`content`](Parameter::content).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Schema>,

    /// A single example of the parameter value.
    ///
    /// Mutually exclusive with [`examples`](Parameter::examples).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,

    /// Examples of the parameter value.
    ///
    /// Mutually exclusive with [`example`](Parameter::example).
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub examples: Map<RefOr<Example>>,

    /// The parameter value described by media type.
    ///
    /// Mutually exclusive with [`schema`](Parameter::schema). The map must hold
    /// exactly one entry.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub content: Map<MediaType>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Parameter {
    /// Creates a schema-described parameter.
    pub fn new(name: impl Into<String>, location: ParameterIn, schema: Schema) -> Self {
        Self {
            name: name.into(),
            location,
            // A path parameter is required by definition, so filling this in is
            // a correctness measure rather than a convenience.
            required: (location == ParameterIn::Path).then_some(true),
            schema: Some(schema),
            ..Self::default()
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
    #[must_use]
    pub fn with_style(mut self, style: Style, explode: bool) -> Self {
        self.style = Some(style);
        self.explode = Some(explode);
        self
    }

    /// Returns the effective style, falling back to the location's default.
    #[must_use]
    pub fn effective_style(&self) -> Style {
        self.style
            .unwrap_or_else(|| Style::default_for(self.location))
    }

    /// Returns the effective explode flag, falling back to the style's default.
    #[must_use]
    pub fn effective_explode(&self) -> bool {
        self.explode
            .unwrap_or_else(|| self.effective_style().default_explode())
    }
}

#[cfg(test)]
mod tests;
