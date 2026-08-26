//! Sequences, sets and maps.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::schema::{MapKey, Schema, impls::with_object, registry::Registry};

/// An array schema over `T`, optionally requiring its members to be distinct.
fn array<T: Schema>(registry: &mut Registry, unique: bool) -> OpenApiSchema {
    let items = registry.resolve::<T>();
    with_object(OpenApiSchema::of_type(SchemaType::Array), |object| {
        object.items = Some(Box::new(items));
        if unique {
            object.unique_items = Some(true);
        }
    })
}

/// An object schema whose values are `V` and whose keys are `K`.
///
/// `propertyNames` is built here as a string schema plus `K`'s constraints,
/// rather than taken from `K`'s own schema — so a key type cannot describe
/// itself as something a JSON object key could never be. It is omitted when
/// `K` constrains nothing, since `{"type": "string"}` says no more than
/// `type: object` already does.
fn map<K: MapKey, V: Schema>(registry: &mut Registry) -> OpenApiSchema {
    let values = registry.resolve::<V>();
    let constraints = K::key_constraints();

    with_object(OpenApiSchema::of_type(SchemaType::Object), |object| {
        object.additional_properties = Some(Box::new(values));
        if !constraints.is_empty() {
            object.property_names = Some(Box::new(
                constraints.apply(OpenApiSchema::of_type(SchemaType::String)),
            ));
        }
    })
}

impl<T: Schema> Schema for Vec<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, false)
    }
}

impl<T: Schema> Schema for VecDeque<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, false)
    }
}

impl<T: Schema> Schema for [T] {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, false)
    }
}

/// A fixed-length array, whose length is part of the contract.
impl<T: Schema, const N: usize> Schema for [T; N] {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        with_object(array::<T>(registry, false), |object| {
            let length = u64::try_from(N).unwrap_or(u64::MAX);
            object.min_items = Some(length);
            object.max_items = Some(length);
        })
    }
}

impl<T: Schema, S> Schema for HashSet<T, S> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, true)
    }
}

impl<T: Schema> Schema for BTreeSet<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, true)
    }
}

impl<K: MapKey, V: Schema, S> Schema for HashMap<K, V, S> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        map::<K, V>(registry)
    }
}

impl<K: MapKey, V: Schema> Schema for BTreeMap<K, V> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        map::<K, V>(registry)
    }
}
