//! The Parameter, Header and Style Objects.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map, body::MediaType, example::Example, extensions::Extensions, reference::RefOr,
    schema::Schema,
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

/// How a parameter value is serialized.
///
/// Not every combination of style and location is legal; OpenAPI 3.2 states
/// that the table of valid combinations is closed. [`crate::validate`] checks
/// the pairing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Style {
    /// Path-style parameters defined by RFC 6570. Path only.
    Matrix,
    /// Label-style expansion defined by RFC 6570. Path only.
    Label,
    /// Comma-separated values. The default for path and header.
    Simple,
    /// Form-style expansion. The default for query and cookie.
    Form,
    /// Space-separated array or object values. Query only.
    SpaceDelimited,
    /// Pipe-separated array or object values. Query only.
    PipeDelimited,
    /// Nested objects rendered as `param[prop]=value`.
    ///
    /// Query only, and defined only for objects whose properties are scalars.
    /// Anything deeper needs [`ParameterIn::Querystring`].
    DeepObject,
    /// Cookie-style serialization.
    ///
    /// Introduced in OpenAPI 3.2. Cookie only.
    #[cfg(feature = "openapi32")]
    Cookie,
}

impl Style {
    /// The style applied when none is stated, given a parameter location.
    #[must_use]
    pub fn default_for(location: ParameterIn) -> Self {
        match location {
            ParameterIn::Query | ParameterIn::Cookie => Self::Form,
            ParameterIn::Path | ParameterIn::Header => Self::Simple,
            #[cfg(feature = "openapi32")]
            ParameterIn::Querystring => Self::Form,
        }
    }

    /// Whether this style may be used at the given location.
    #[must_use]
    pub fn is_valid_for(self, location: ParameterIn) -> bool {
        match self {
            Self::Matrix | Self::Label => location == ParameterIn::Path,
            Self::Simple => matches!(location, ParameterIn::Path | ParameterIn::Header),
            Self::Form => matches!(location, ParameterIn::Query | ParameterIn::Cookie),
            Self::SpaceDelimited | Self::PipeDelimited | Self::DeepObject => {
                location == ParameterIn::Query
            }
            #[cfg(feature = "openapi32")]
            Self::Cookie => location == ParameterIn::Cookie,
        }
    }

    /// Whether `explode` defaults to `true` for this style.
    #[must_use]
    pub fn default_explode(self) -> bool {
        self == Self::Form
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

/// Header names that must not be declared as [`ParameterIn::Header`]
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
/// This is a Parameter Object without `name` and `in`. Its
/// [`style`](Header::style), when present, must be [`Style::Simple`], and
/// `allowEmptyValue` must not be used.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// A description of the header. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether the header is mandatory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// Whether the header is deprecated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,

    /// How the value is serialized. Must be [`Style::Simple`] when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Style>,

    /// Whether an array or object generates one value per member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explode: Option<bool>,

    /// The schema of the header value.
    ///
    /// Mutually exclusive with [`content`](Header::content).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Schema>,

    /// A single example of the header value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,

    /// Examples of the header value.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub examples: Map<RefOr<Example>>,

    /// The header value described by media type.
    ///
    /// Mutually exclusive with [`schema`](Header::schema). Exactly one entry.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub content: Map<MediaType>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Header {
    /// Creates a header described by a schema.
    pub fn new(schema: Schema) -> Self {
        Self {
            schema: Some(schema),
            ..Self::default()
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

#[cfg(test)]
mod tests {
    use super::{Parameter, ParameterIn, Style, is_ignored_header_parameter};
    use crate::schema::{Schema, SchemaType};

    #[test]
    fn path_parameters_are_required_on_construction() {
        let parameter = Parameter::path("id", Schema::of_type(SchemaType::String));
        assert_eq!(parameter.required, Some(true));
    }

    #[test]
    fn query_parameters_are_not_required_by_default() {
        let parameter = Parameter::query("page", Schema::of_type(SchemaType::Integer));
        assert_eq!(parameter.required, None);
    }

    #[test]
    fn style_defaults_follow_the_parameter_location() {
        assert_eq!(Style::default_for(ParameterIn::Query), Style::Form);
        assert_eq!(Style::default_for(ParameterIn::Path), Style::Simple);
        assert_eq!(Style::default_for(ParameterIn::Header), Style::Simple);
        assert_eq!(Style::default_for(ParameterIn::Cookie), Style::Form);
    }

    #[test]
    fn the_style_location_table_is_closed() {
        assert!(Style::Matrix.is_valid_for(ParameterIn::Path));
        assert!(!Style::Matrix.is_valid_for(ParameterIn::Query));
        assert!(Style::DeepObject.is_valid_for(ParameterIn::Query));
        assert!(!Style::DeepObject.is_valid_for(ParameterIn::Path));
        assert!(!Style::Form.is_valid_for(ParameterIn::Header));
    }

    #[test]
    fn explode_defaults_to_true_only_for_form() {
        assert!(Style::Form.default_explode());
        assert!(!Style::Simple.default_explode());
    }

    #[test]
    fn effective_style_and_explode_fall_back_to_defaults() {
        let parameter = Parameter::query("tags", Schema::of_type(SchemaType::Array));
        assert_eq!(parameter.effective_style(), Style::Form);
        assert!(parameter.effective_explode());
    }

    #[test]
    fn headers_the_spec_ignores_are_recognized_case_insensitively() {
        assert!(is_ignored_header_parameter("Authorization"));
        assert!(is_ignored_header_parameter("content-type"));
        assert!(is_ignored_header_parameter("ACCEPT"));
        assert!(!is_ignored_header_parameter("X-Request-Id"));
    }
}
