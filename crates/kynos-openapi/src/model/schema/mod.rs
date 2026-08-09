//! The Schema Object: JSON Schema 2020-12 plus the OAS base vocabulary.

pub mod dialect;
pub mod discriminator;
pub mod object;
pub mod types;
pub mod xml;

use serde::{Deserialize, Serialize};

use crate::model::schema::{
    object::SchemaObject,
    types::{SchemaType, TypeSet},
};

/// A JSON Schema.
///
/// A boolean is a valid schema in JSON Schema 2020-12: `true` accepts every
/// instance and `false` accepts none. That is why this is an enum rather than a
/// struct.
///
/// [`Schema::Bool(true)`](Schema::Bool) is how a genuinely unconstrained
/// payload is represented. Kynos never produces it by accident — a Rust type
/// that cannot describe itself has no `Schema` implementation at all, and the
/// permissive schema is reachable only by naming it in the handler signature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Schema {
    /// The trivially true (`true`) or trivially false (`false`) schema.
    Bool(bool),
    /// A schema with keywords.
    Object(Box<SchemaObject>),
}

impl Default for Schema {
    fn default() -> Self {
        Self::Object(Box::default())
    }
}

impl Schema {
    /// The schema that accepts any instance.
    #[must_use]
    pub fn any() -> Self {
        Self::Bool(true)
    }

    /// The schema that accepts no instance.
    #[must_use]
    pub fn never() -> Self {
        Self::Bool(false)
    }

    /// A schema constrained to a single primitive type.
    #[must_use]
    pub fn of_type(ty: SchemaType) -> Self {
        Self::Object(Box::new(SchemaObject {
            ty: Some(TypeSet::One(ty)),
            ..SchemaObject::default()
        }))
    }

    /// A schema that is `ty` or `null`.
    ///
    /// This is how nullability is expressed from OpenAPI 3.1 onward. The 3.0
    /// `nullable: true` keyword does not exist and must never be emitted.
    #[must_use]
    pub fn nullable(ty: SchemaType) -> Self {
        Self::Object(Box::new(SchemaObject {
            ty: Some(TypeSet::Many(vec![ty, SchemaType::Null])),
            ..SchemaObject::default()
        }))
    }

    /// A `$ref` to another schema.
    ///
    /// Unlike a [`Ref`](crate::Ref), sibling keywords on a schema `$ref` are
    /// applied rather than ignored.
    #[must_use]
    pub fn reference(uri: impl Into<String>) -> Self {
        Self::Object(Box::new(SchemaObject {
            reference: Some(uri.into()),
            ..SchemaObject::default()
        }))
    }

    /// A `$ref` to a named entry under `#/components/schemas`.
    #[must_use]
    pub fn component(name: &str) -> Self {
        Self::reference(format!("#/components/schemas/{name}"))
    }

    /// Returns the keyword-carrying form, if this is not a boolean schema.
    #[must_use]
    pub fn as_object(&self) -> Option<&SchemaObject> {
        match self {
            Self::Object(object) => Some(object),
            Self::Bool(_) => None,
        }
    }
}

#[cfg(test)]
mod tests;
