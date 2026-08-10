//! Transparent wrappers, and the one that adds nullability.

use std::sync::Arc;

use kynos_openapi::{
    ComponentName, Schema as OpenApiSchema,
    model::schema::types::{SchemaType, TypeSet},
};

use crate::schema::{Schema, impls::with_object, registry::Registry};

/// Widens `schema` to admit `null`.
///
/// A schema that already says `type: <one thing>` and nothing referential just
/// gains `null` to its type union, which is how JSON Schema has expressed
/// nullability since 2020-12. Anything else — a `$ref`, a union, a composed
/// schema — goes under an `anyOf`, because widening a `$ref` in place would
/// mean editing the type it points at.
pub(crate) fn nullable(schema: OpenApiSchema) -> OpenApiSchema {
    // A schema that already admits `null` is as nullable as it can be, and a
    // type union's members must be unique — so `Option<()>` and
    // `Option<Option<T>>` widen to themselves rather than to a repeat.
    if admits_null(&schema) {
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

/// Whether `schema` already accepts the `null` instance by way of its `type`.
fn admits_null(schema: &OpenApiSchema) -> bool {
    schema
        .as_object()
        .and_then(|object| object.ty.as_ref())
        .is_some_and(|ty| match ty {
            TypeSet::One(one) => *one == SchemaType::Null,
            TypeSet::Many(many) => many.contains(&SchemaType::Null),
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
            }
        )+
    };
}

// `name` delegates too: a `Box<User>` and a `User` are the same component, and
// registering them separately would put the same schema in the document twice.
transparent!(Box, Arc);
