//! The Header Object, and the headers the specification refuses to describe.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Map,
    model::{
        body::media_type::MediaType, example::Example, extensions::Extensions,
        parameter::style::Style, reference::RefOr, schema::Schema,
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
