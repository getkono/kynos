//! The External Documentation Object.

use serde::{Deserialize, Serialize};

use crate::extensions::Extensions;

/// A pointer to extended documentation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalDocumentation {
    /// A description of the target documentation. [CommonMark] syntax may be
    /// used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The URI for the target documentation.
    pub url: String,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl ExternalDocumentation {
    /// Points at the given URI.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}
