//! Saying in the type system that a payload is deliberately unconstrained.

use kynos_openapi::{
    Schema as OpenApiSchema, SchemaObject, annotation::UNCHECKED_SCHEMA_ANNOTATION,
};

use crate::schema::{
    Schema,
    flatten::{AdmitsAny, OpenMap},
    registry::Registry,
};

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
///
/// # Flattening
///
/// Arbitrary JSON beside the members an object declares is an `Unchecked` map
/// under `#[serde(flatten)] #[schema(open)]`. It is an
/// [`OpenMap`] over a `serde_json::Map<String, Value>`,
/// or over a payload that is an `OpenMap` itself:
///
/// ```
/// fn open<T: kynos::schema::flatten::OpenMap>() {}
///
/// open::<kynos::schema::unchecked::Unchecked<serde_json::Map<String, serde_json::Value>>>();
/// open::<kynos::schema::unchecked::Unchecked<std::collections::BTreeMap<String, u64>>>();
/// ```
///
/// A payload that is not a map is refused here, rather than by serde at run
/// time with "can only flatten structs and maps". A struct payload needs no
/// wrapper: flatten the struct itself.
///
/// ```compile_fail
/// fn open<T: kynos::schema::flatten::OpenMap>() {}
///
/// open::<kynos::schema::unchecked::Unchecked<u64>>();
/// ```
///
/// It is never [`Flatten`](crate::schema::flatten::Flatten). The permissive schema names
/// no member it contributes, so beside an open map those members stay
/// unevaluated and the map's `unevaluatedProperties` would refuse what serde
/// writes:
///
/// ```compile_fail
/// fn flattenable<T: kynos::schema::flatten::Flatten>() {}
///
/// flattenable::<kynos::schema::unchecked::Unchecked<serde_json::Map<String, serde_json::Value>>>();
/// ```
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

/// Flattened open beside the members an object declares: the schema is written
/// in place and carries no `additionalProperties`, so the hoist moves nothing
/// and the object is left open. Bounded by `T`'s own `OpenMap`, because serde
/// flattens only structs and maps.
impl<T: OpenMap> OpenMap for Unchecked<T> {}

/// `serde_json::Map` has no [`Schema`] of its own, so it reaches [`OpenMap`] only
/// through `Unchecked`.
impl OpenMap for Unchecked<serde_json::Map<String, serde_json::Value>> {}

/// Hoists nothing wherever it is an [`OpenMap`], since the permissive schema has
/// no `additionalProperties`, so a field serde writes and never reads may sit
/// beside it.
impl<T> AdmitsAny for Unchecked<T> where Self: OpenMap {}
