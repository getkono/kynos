//! The Example Object.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::extensions::Extensions;

/// A worked example of a parameter, request body, response body or header.
///
/// # Choosing a value field
///
/// OpenAPI 3.1 offers [`value`](Example::value) and
/// [`external_value`](Example::external_value), which are mutually exclusive.
///
/// 3.2 adds [`data_value`](Example::data_value) and
/// [`serialized_value`](Example::serialized_value), and deprecates `value` for
/// non-JSON serialization targets — for those, `value` has
/// implementation-defined behaviour, which is exactly the kind of ambiguity
/// Kynos avoids. Prefer `data_value` (the example as data, before
/// serialization) and `serialized_value` (the example as it appears on the
/// wire) whenever `openapi32` is available and the target is not JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Example {
    /// A short description of the example.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A long description of the example. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The embedded example, before serialization.
    ///
    /// Mutually exclusive with [`external_value`](Example::external_value). See
    /// the type-level documentation before using this with a non-JSON target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,

    /// The example as data, prior to serialization.
    ///
    /// Introduced in OpenAPI 3.2. Mutually exclusive with
    /// [`value`](Example::value).
    #[cfg(feature = "openapi32")]
    #[serde(rename = "dataValue", default, skip_serializing_if = "Option::is_none")]
    pub data_value: Option<Value>,

    /// The example exactly as it appears on the wire.
    ///
    /// Introduced in OpenAPI 3.2. Mutually exclusive with
    /// [`value`](Example::value) and
    /// [`external_value`](Example::external_value).
    #[cfg(feature = "openapi32")]
    #[serde(
        rename = "serializedValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub serialized_value: Option<String>,

    /// A URI identifying a literal example, for payloads that cannot be
    /// embedded in JSON or YAML.
    #[serde(
        rename = "externalValue",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub external_value: Option<String>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Example {
    /// Creates an example holding an embedded value.
    pub fn new(value: impl Into<Value>) -> Self {
        Self {
            value: Some(value.into()),
            ..Self::default()
        }
    }

    /// Creates an example pointing at an external payload.
    pub fn external(uri: impl Into<String>) -> Self {
        Self {
            external_value: Some(uri.into()),
            ..Self::default()
        }
    }

    /// Sets the short summary.
    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Sets the long description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}
