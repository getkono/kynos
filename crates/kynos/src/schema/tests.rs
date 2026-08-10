use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use kynos_openapi::{
    Schema as OpenApiSchema, SchemaObject,
    model::schema::types::{SchemaType, TypeSet},
};

use crate::schema::{MapKey, Schema, registry::Registry};

/// The registry is only touched by implementations that have members to
/// resolve, so the ones checked here can be driven without one being built.
fn schema_of<T: Schema>() -> OpenApiSchema {
    T::schema(&mut Registry::default())
}

fn object_of<T: Schema>() -> SchemaObject {
    match schema_of::<T>() {
        OpenApiSchema::Object(object) => *object,
        OpenApiSchema::Bool(_) => panic!("expected a keyword-carrying schema"),
    }
}

/// Proves an implementation exists without running it, which is the only way
/// to reach the ones that resolve their members while the registry is a
/// placeholder.
fn describable<T: Schema>() {}

fn keyable<T: MapKey>() {}

#[test]
fn primitives_carry_their_type_and_format() {
    assert_eq!(
        object_of::<bool>().ty,
        Some(TypeSet::One(SchemaType::Boolean))
    );
    assert_eq!(
        object_of::<String>().ty,
        Some(TypeSet::One(SchemaType::String))
    );
    assert_eq!(object_of::<i32>().format.as_deref(), Some("int32"));
    assert_eq!(object_of::<i64>().format.as_deref(), Some("int64"));
    assert_eq!(object_of::<f32>().format.as_deref(), Some("float"));
    assert_eq!(object_of::<f64>().format.as_deref(), Some("double"));
}

#[test]
fn integer_bounds_are_the_types_own() {
    assert_eq!(object_of::<u8>().minimum, Some(0.0));
    assert_eq!(object_of::<u8>().maximum, Some(255.0));
    assert_eq!(object_of::<i8>().minimum, Some(-128.0));
    assert_eq!(object_of::<i8>().maximum, Some(127.0));
    assert_eq!(object_of::<u32>().maximum, Some(4_294_967_295.0));
}

#[test]
fn the_wide_integers_state_no_bound_they_cannot_state_exactly() {
    // `i64::MAX` and `u64::MAX` do not survive a round trip through `f64`, so
    // emitting them would forbid values the type accepts.
    assert_eq!(object_of::<i64>().minimum, None);
    assert_eq!(object_of::<i64>().maximum, None);
    assert_eq!(object_of::<u64>().minimum, Some(0.0));
    assert_eq!(object_of::<u64>().maximum, None);
}

#[test]
fn a_char_is_a_string_of_exactly_one() {
    let object = object_of::<char>();
    assert_eq!(object.ty, Some(TypeSet::One(SchemaType::String)));
    assert_eq!(object.min_length, Some(1));
    assert_eq!(object.max_length, Some(1));
}

#[test]
fn the_unit_type_is_null() {
    assert_eq!(object_of::<()>().ty, Some(TypeSet::One(SchemaType::Null)));
}

#[test]
fn addresses_use_their_named_formats() {
    assert_eq!(object_of::<Ipv4Addr>().format.as_deref(), Some("ipv4"));
    assert_eq!(object_of::<Ipv6Addr>().format.as_deref(), Some("ipv6"));

    let either = object_of::<IpAddr>();
    assert_eq!(either.ty, None);
    assert_eq!(either.any_of.map(|branches| branches.len()), Some(2));
}

#[test]
fn every_standard_type_the_docs_promise_is_describable() {
    describable::<bool>();
    describable::<String>();
    describable::<char>();
    describable::<i8>();
    describable::<i16>();
    describable::<i32>();
    describable::<i64>();
    describable::<u8>();
    describable::<u16>();
    describable::<u32>();
    describable::<u64>();
    describable::<f32>();
    describable::<f64>();
    describable::<()>();

    describable::<Option<String>>();
    describable::<Box<u32>>();
    describable::<Arc<u32>>();

    describable::<Vec<String>>();
    describable::<VecDeque<String>>();
    describable::<[u8; 4]>();
    describable::<HashSet<String>>();
    describable::<BTreeSet<String>>();
    describable::<HashMap<String, u32>>();
    describable::<BTreeMap<String, u32>>();

    describable::<(u32,)>();
    describable::<(u32, String)>();
    describable::<(
        u32,
        String,
        bool,
        f64,
        char,
        u8,
        i8,
        u16,
        i16,
        u32,
        i32,
        i64,
    )>();

    describable::<Ipv4Addr>();
    describable::<Ipv6Addr>();
    describable::<IpAddr>();

    // Nesting is what makes the set useful, and it must terminate.
    describable::<Vec<Option<HashMap<String, Vec<u32>>>>>();
}

#[test]
fn a_string_is_a_map_key() {
    keyable::<String>();
}

/// `Box<T>` and `Arc<T>` are `T` on the wire, so they must not mint a second
/// component name for the same schema.
#[test]
fn transparent_wrappers_share_the_inner_component_name() {
    assert_eq!(<Box<u32> as Schema>::name(), <u32 as Schema>::name());
    assert_eq!(<Arc<u32> as Schema>::name(), <u32 as Schema>::name());
}

/// Anonymous types inline; only named ones are referenced.
#[test]
fn standard_types_are_anonymous() {
    assert!(<u32 as Schema>::name().is_none());
    assert!(<Vec<String> as Schema>::name().is_none());
    assert!(<(u32, u32) as Schema>::name().is_none());
}
