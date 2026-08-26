//! Saying in the type system that a payload is deliberately unconstrained.

use kynos_openapi::{
    Schema as OpenApiSchema, SchemaObject, annotation::UNCHECKED_SCHEMA_ANNOTATION,
};

use crate::schema::{Schema, registry::Registry};

/// A payload this API deliberately does not constrain.
///
/// Wrapping a type in `Unchecked` emits the permissive JSON Schema (`true`)
/// annotated with `x-kynos-unchecked`, so a consumer reading the description
/// can see that the shape is unspecified rather than merely undocumented.
///
/// Use it when the payload genuinely is arbitrary — a passthrough proxy, a
/// webhook envelope whose body belongs to a third party. Do not use it to avoid
/// writing a type.
///
/// `Router::deny_unchecked_schemas` turns the resulting warning into a build
/// error, for teams that want to forbid it outright.
/// Transparent to serde, because the annotation is a fact about the
/// description and not about the encoding. `Unchecked<T>` and `T` are the same
/// bytes, so wrapping a field costs a consumer nothing — and a wrapper that did
/// reach the wire would make the only sanctioned way to carry an arbitrary
/// payload the one way that changes its shape.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct Unchecked<T>(pub T);

impl<T> Unchecked<T> {
    /// Unwraps the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Schema for Unchecked<T> {
    /// The permissive schema, carrying the annotation.
    ///
    /// Written with keywords rather than as `true`, because a boolean schema
    /// has nowhere to put one — and a keyword set that constrains nothing is
    /// the same schema `true` is. `T` is not consulted: whatever it is, the
    /// point of this wrapper is that the description does not claim its shape.
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        let mut object = SchemaObject::default();
        object
            .unknown_keywords
            .insert(UNCHECKED_SCHEMA_ANNOTATION.to_owned(), true.into());
        OpenApiSchema::Object(Box::new(object))
    }
}
