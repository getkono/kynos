//! Saying in the type system that a payload is deliberately unconstrained.

use kynos_openapi::Schema as OpenApiSchema;

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unchecked<T>(pub T);

impl<T> Unchecked<T> {
    /// Unwraps the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Schema for Unchecked<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        let _ = registry;
        todo!()
    }
}
