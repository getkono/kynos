//! Sequences, sets and maps.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use kynos_openapi::{Schema as OpenApiSchema, model::schema::types::SchemaType};

use crate::{
    __private::constraints as keyword,
    schema::{
        MapKey, Schema,
        constraints::{Constraints, Pointer, Violations},
        flatten::{AdmitsAny, OpenMap},
        impls::with_object,
        registry::Registry,
        unchecked::Unchecked,
    },
};

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

/// Checks each element of a sequence at its index.
fn check_elements<'a, T: Schema + 'a>(
    elements: impl Iterator<Item = &'a T>,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    for (index, element) in elements.enumerate() {
        element.check_constraints(at.index(index), violations);
    }
}

/// Checks each member of a set, at the set: its iteration order is not the
/// order the document listed it in, so no index would name it.
fn check_members<'a, T: Schema + 'a>(
    members: impl Iterator<Item = &'a T>,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    violations.within(at, |inner| {
        for member in members {
            member.check_constraints(Pointer::root(), inner);
        }
    });
}

/// Checks each entry of a map: its key against the `propertyNames` that `K`
/// declares, then its value under the key.
///
/// A key has no location of its own, since a pointer to it names its value,
/// so a key is reported at the map, the key in the detail. Only the length
/// bounds can fail: a key is a string, and a key's `pattern` is described and
/// not checked. A key that cannot say what member name it is is
/// not checked, and its value is reported at the map.
///
/// `K::key_constraints` is built once the first key to check needs it, and
/// only its length bounds are kept, so an empty map, or one whose keys cannot
/// be checked, builds nothing.
fn check_entries<'a, K: MapKey + 'a, V: Schema + 'a>(
    entries: impl Iterator<Item = (&'a K, &'a V)>,
    at: Pointer<'_>,
    violations: &mut Violations,
) {
    let mut lengths = None;
    for (key, value) in entries {
        match key.as_member() {
            Some(name) => {
                let lengths = *lengths.get_or_insert_with(|| KeyLengths::of::<K>());
                check_key(name, lengths, at, violations);
                value.check_constraints(at.member(name), violations);
            }
            None => violations.within(at, |inner| {
                value.check_constraints(Pointer::root(), inner);
            }),
        }
    }
}

/// The bounds of a map key that a check enforces.
#[derive(Clone, Copy)]
struct KeyLengths {
    min: Option<u64>,
    max: Option<u64>,
}

impl KeyLengths {
    fn of<K: MapKey>() -> Self {
        let Constraints {
            min_length,
            max_length,
            ..
        } = K::key_constraints();
        Self {
            min: min_length,
            max: max_length,
        }
    }
}

/// Checks one key against its `lengths`, reporting at `at`, the map.
fn check_key(name: &str, lengths: KeyLengths, at: Pointer<'_>, violations: &mut Violations) {
    let mut broken = Violations::new();
    if let Some(bound) = lengths.min {
        keyword::text_min_length(name, bound, Pointer::root(), &mut broken);
    }
    if let Some(bound) = lengths.max {
        keyword::text_max_length(name, bound, Pointer::root(), &mut broken);
    }
    for (_, detail) in broken.into_each() {
        violations.report(at, format!("the key `{name}` {detail}"));
    }
}

impl<T: Schema> Schema for Vec<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, false)
    }

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        check_elements(self.iter(), at, violations);
    }
}

impl<T: Schema> Schema for VecDeque<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, false)
    }

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        check_elements(self.iter(), at, violations);
    }
}

impl<T: Schema> Schema for [T] {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, false)
    }

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        check_elements(self.iter(), at, violations);
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

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        check_elements(self.iter(), at, violations);
    }
}

impl<T: Schema, S> Schema for HashSet<T, S> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, true)
    }

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        check_members(self.iter(), at, violations);
    }
}

impl<T: Schema> Schema for BTreeSet<T> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        array::<T>(registry, true)
    }

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        check_members(self.iter(), at, violations);
    }
}

impl<K: MapKey, V: Schema, S> Schema for HashMap<K, V, S> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        map::<K, V>(registry)
    }

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        check_entries(self.iter(), at, violations);
    }
}

impl<K: MapKey, V: Schema> Schema for BTreeMap<K, V> {
    fn schema(registry: &mut Registry) -> OpenApiSchema {
        map::<K, V>(registry)
    }

    fn check_constraints(&self, at: Pointer<'_>, violations: &mut Violations) {
        check_entries(self.iter(), at, violations);
    }
}

// A map claims no component name, so `resolve` hands back `map`'s object
// itself -- the `additionalProperties` that `#[schema(open)]` hoists.
impl<K: MapKey, V: Schema, S> OpenMap for HashMap<K, V, S> {}

impl<K: MapKey, V: Schema> OpenMap for BTreeMap<K, V> {}

// The hoisted `additionalProperties` is the value schema, so a map constrains
// no member exactly when that schema admits every value. `Unchecked`'s is the
// permissive one whatever it wraps, so the map is bounded by its value's type
// rather than by a marker a value type could claim while describing itself
// with a constraint. The key needs no bound beyond `MapKey`: the hoist drops
// `propertyNames`, so no key constraint reaches the object.
impl<K: MapKey, V, S> AdmitsAny for HashMap<K, Unchecked<V>, S> {}

impl<K: MapKey, V> AdmitsAny for BTreeMap<K, Unchecked<V>> {}
