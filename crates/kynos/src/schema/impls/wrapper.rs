//! Transparent wrappers, and the one that adds nullability.

use std::sync::Arc;

use kynos_openapi::{
    ComponentName, Schema as OpenApiSchema,
    model::schema::types::{SchemaType, TypeSet},
};

use crate::schema::{
    Schema,
    constraints::{Pointer, Violations},
    flatten::{AdmitsAny, ClosedFlatten, Flatten, OpenMap},
    impls::with_object,
    registry::Registry,
    type_admits_null,
};

/// Widens `schema` to admit `null`.
///
/// A single `type` with no `$ref` gains `null` in its type union; anything
/// else goes under an `anyOf`, since widening a `$ref` in place would edit its
/// target.
pub(crate) fn nullable(schema: OpenApiSchema) -> OpenApiSchema {
    // Type union members must be unique, so `Option<Option<T>>` widens to itself.
    if type_admits_null(&schema) {
        return schema;
    }

    let promotable = schema.as_object().is_some_and(|object| {
        object.reference.is_none() && matches!(object.ty, Some(TypeSet::One(_)))
    });

    if promotable {
        return with_object(schema, |object| {
            if let Some(TypeSet::One(ty)) = object.ty {
                object.ty = Some(TypeSet::Many(vec![ty, SchemaType::Null]));
            }
        });
    }

    with_object(OpenApiSchema::default(), |object| {
        object.any_of = Some(vec![schema, OpenApiSchema::of_type(SchemaType::Null)]);
    })
}

/// A value that may be absent, described as one that may be `null`.
///
/// Whether an *object field* of this type is also optional is a separate
/// question, answered by the `required` list the derive builds.
impl<T: Schema> Schema for Option<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        nullable(registry.resolve::<T>())
    }

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        if let Some(value) = self {
            value.check_constraints(at, violations);
        }
    }
}

/// Emits a delegating implementation for a wrapper with no wire form of its own.
macro_rules! transparent {
    ($($ty:ident),+ $(,)?) => {
        $(
            impl<T: Schema> Schema for $ty<T> {
                fn schema(registry: &mut Registry) -> OpenApiSchema {
                    T::schema(registry)
                }

                fn name() -> Option<ComponentName> {
                    T::name()
                }

                fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
                    T::check_constraints(self, at, violations);
                }
            }
        )+
    };
}

// `name` delegates too, so a `Box<User>` and a `User` are one component.
transparent!(Box, Arc);

// The flatten markers carry across as `T`'s answer. Written out rather than in
// `transparent!`, so rustc's unsatisfied-trait notes do not name the macro.
impl<T: Flatten> Flatten for Box<T> {}

impl<T: Flatten> Flatten for Arc<T> {}

impl<T: ClosedFlatten> ClosedFlatten for Box<T> {}

impl<T: ClosedFlatten> ClosedFlatten for Arc<T> {}

impl<T: OpenMap> OpenMap for Box<T> {}

impl<T: OpenMap> OpenMap for Arc<T> {}

impl<T: AdmitsAny> AdmitsAny for Box<T> {}

impl<T: AdmitsAny> AdmitsAny for Arc<T> {}
