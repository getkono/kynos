//! The Style Object, and the closed style/location table.

use serde::{Deserialize, Serialize};

use crate::model::parameter::ParameterIn;

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

/// The one style a header may declare.
///
/// A [`Style`] narrowed to the value the specification leaves legal. A header
/// has no `in` field for a style to disagree with, so the restriction is not a
/// pairing between two fields but a domain: one variant, and a description
/// naming any other style does not parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HeaderStyle {
    /// Comma-separated values, defined by RFC 6570.
    Simple,
}

impl From<HeaderStyle> for Style {
    fn from(_: HeaderStyle) -> Self {
        Self::Simple
    }
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
