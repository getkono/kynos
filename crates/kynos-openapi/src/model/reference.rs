//! The Reference Object, and the "either a reference or the thing" wrapper.

use serde::{Deserialize, Serialize};

/// A reference to another part of this or another description.
///
/// # This is not a JSON Schema `$ref`
///
/// A *Reference Object* has exactly three fields, and any other property beside
/// them **shall be ignored**, so it carries no
/// [`Extensions`](crate::Extensions). A Schema Object `$ref` is JSON Schema
/// 2020-12, where siblings apply, so [`Schema`](crate::Schema) does not use
/// this type.
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

/// Why a `$ref` could not be followed to the object it stands for.
///
/// Returned by [`Document::resolve`](crate::Document::resolve) and
/// [`Document::resolve_schema`](crate::Document::resolve_schema), which follow
/// only same-document pointers to a component by its literal name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UnresolvedRef {
    /// A reference this document alone cannot follow: anything but a
    /// fragment-only `#/components/...` pointer, an absolute URI naming this
    /// same document included; or one below a component, into another
    /// section, or naming a component with `~` or `%`, which no valid
    /// component name needs.
    #[error("the reference does not name a component of this document's own section")]
    NotLocal,
    /// A reference to a component this document does not declare.
    #[error("the reference names a component this document does not declare")]
    Missing,
    /// A chain of references that returns to a component it already passed.
    #[error("the reference is part of a cycle of references")]
    Cycle,
}

/// The name `reference` gives a component in `section`, where it is a
/// same-document pointer into `#/components/<section>/`, as written.
///
/// What remains is returned undecoded, so a lookup by it finds a component
/// only under exactly that spelling.
pub(crate) fn component_name<'r>(reference: &'r str, section: &str) -> Option<&'r str> {
    reference
        .strip_prefix("#/components/")?
        .strip_prefix(section)?
        .strip_prefix('/')
}

/// The same, refusing a name holding `/`, `~` or `%`: such a pointer is
/// escaped or reaches below the component, and no valid component name
/// needs any of the three.
pub(crate) fn literal_component_name<'r>(reference: &'r str, section: &str) -> Option<&'r str> {
    component_name(reference, section).filter(|name| !name.contains(['/', '~', '%']))
}

impl<T> From<Ref> for RefOr<T> {
    fn from(reference: Ref) -> Self {
        Self::Ref(reference)
    }
}

#[cfg(test)]
mod tests;
