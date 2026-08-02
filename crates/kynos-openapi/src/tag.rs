//! The Tag Object.

use serde::{Deserialize, Serialize};

use crate::{extensions::Extensions, external_docs::ExternalDocumentation};

/// Metadata for a single tag used by [`Operation::tags`](crate::Operation::tags).
///
/// Tag names must be unique across a document. In Kynos a tag is a *type*
/// rather than a string, so uniqueness is a property of the type system rather
/// than something checked after the fact.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    /// The name of the tag. Operations refer to it by this value.
    pub name: String,

    /// A short summary of the tag.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A description for the tag. [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The [`name`](Tag::name) of a tag that this tag nests under.
    ///
    /// Introduced in OpenAPI 3.2. The named tag must exist, and the parent
    /// chain must not contain a cycle; [`crate::validate`] checks both.
    #[cfg(feature = "openapi32")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,

    /// A machine-readable categorization of what sort of tag this is.
    ///
    /// Introduced in OpenAPI 3.2. Any string is permitted; `nav`, `badge` and
    /// `audience` are the common registered values.
    #[cfg(feature = "openapi32")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Additional external documentation for this tag.
    #[serde(
        rename = "externalDocs",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub external_docs: Option<ExternalDocumentation>,

    /// Specification extensions.
    #[serde(flatten)]
    pub extensions: Extensions,
}

impl Tag {
    /// Creates a tag with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Sets the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Nests this tag under another.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[must_use]
    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        self.parent = Some(parent.into());
        self
    }

    /// Sets the machine-readable tag category.
    ///
    /// Introduced in OpenAPI 3.2.
    #[cfg(feature = "openapi32")]
    #[must_use]
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }
}
