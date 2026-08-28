//! The Reference Object, and the "either a reference or the thing" wrapper.

use serde::{Deserialize, Serialize};

/// A reference to another part of this or another description.
///
/// # This is not a JSON Schema `$ref`
///
/// The specification draws a sharp line that is easy to miss. A *Reference
/// Object* — this type — has exactly three fields, and any other property
/// present alongside them **shall be ignored**. A *Schema Object* `$ref` is
/// plain JSON Schema 2020-12, where sibling keywords are fully applied.
///
/// [`Schema`](crate::Schema) therefore models `$ref` as an ordinary keyword,
/// and does not use this type.
///
/// Accordingly this object carries no [`Extensions`](crate::Extensions): it
/// cannot be extended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ref {
    /// The URI of the referenced component.
    #[serde(rename = "$ref")]
    pub location: String,

    /// A short summary, overriding that of the referenced component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// A description, overriding that of the referenced component.
    ///
    /// [CommonMark] syntax may be used.
    ///
    /// [CommonMark]: https://spec.commonmark.org/
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Ref {
    /// References an arbitrary URI.
    pub fn new(location: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            summary: None,
            description: None,
        }
    }

    /// References a named entry under `#/components/schemas`.
    #[must_use]
    pub fn schema(name: &str) -> Self {
        Self::new(format!("#/components/schemas/{name}"))
    }

    /// References a named entry under `#/components/responses`.
    #[must_use]
    pub fn response(name: &str) -> Self {
        Self::new(format!("#/components/responses/{name}"))
    }

    /// References a named entry under `#/components/parameters`.
    #[must_use]
    pub fn parameter(name: &str) -> Self {
        Self::new(format!("#/components/parameters/{name}"))
    }

    /// References a named entry under `#/components/requestBodies`.
    #[must_use]
    pub fn request_body(name: &str) -> Self {
        Self::new(format!("#/components/requestBodies/{name}"))
    }

    /// References a named entry under `#/components/securitySchemes`.
    #[must_use]
    pub fn security_scheme(name: &str) -> Self {
        Self::new(format!("#/components/securitySchemes/{name}"))
    }

    /// Sets the overriding summary.
    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Sets the overriding description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Either an inline `T` or a [`Ref`] standing in for one.
///
/// Deserialization prefers the reference: an object carrying `$ref` is always
/// read as a [`Ref`], matching the specification's rule that the remaining
/// properties of a Reference Object are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RefOr<T> {
    /// A reference to a component defined elsewhere.
    Ref(Ref),
    /// The object itself, inline.
    Item(T),
}

impl<T> RefOr<T> {
    /// Returns the inline item, or `None` when this is a reference.
    pub fn as_item(&self) -> Option<&T> {
        match self {
            Self::Item(item) => Some(item),
            Self::Ref(_) => None,
        }
    }

    /// Returns the reference, or `None` when this is an inline item.
    pub fn as_ref_object(&self) -> Option<&Ref> {
        match self {
            Self::Ref(reference) => Some(reference),
            Self::Item(_) => None,
        }
    }

    /// Returns `true` when this is a reference rather than an inline item.
    pub fn is_ref(&self) -> bool {
        matches!(self, Self::Ref(_))
    }
}

impl<T> From<Ref> for RefOr<T> {
    fn from(reference: Ref) -> Self {
        Self::Ref(reference)
    }
}

#[cfg(test)]
mod tests;
