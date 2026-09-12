//! What a flattened field contributes to the object that carries it.
//!
//! A description is only worth anything if the type it came from can produce an
//! instance it accepts. That is the property here, and it is checked the only
//! way that does not restate the emitter: serialize the value, compile the
//! emitted schema with a real JSON Schema validator, and validate one against
//! the other.
//!
//! `#[serde(flatten)]` is where the two halves can disagree without anything
//! else noticing. The flattened field's members become the parent's own, so the
//! parent composes the field's schema rather than naming it — and a composed
//! schema that constrains *every* member of the instance then reaches the
//! members the parent declared itself.

// `test-util` carries the JSON Schema validator, which is what makes this a
// check against an oracle rather than an assertion about the emitter written
// from the emitter.
#![cfg(all(feature = "macros", feature = "test-util"))]

use std::collections::BTreeMap;

use kynos::{Schema, schema::Schema as SchemaTrait};
use serde::Serialize;

/// The schema `T` emits, as JSON.
fn emitted<T: SchemaTrait>() -> serde_json::Value {
    let mut registry = kynos::schema::registry::Registry::new();
    serde_json::to_value(T::schema(&mut registry)).expect("a schema serializes")
}

/// Every way `value` fails the schema its own type emits.
fn refusals<T: SchemaTrait + Serialize>(value: &T) -> Vec<String> {
    let schema = emitted::<T>();
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");
    let instance = serde_json::to_value(value).expect("the value serializes");

    validator
        .iter_errors(&instance)
        .map(|error| error.to_string())
        .collect()
}

/// A struct whose extra members are a map, with the members it declares itself.
#[derive(Schema, Serialize)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    extra: BTreeMap<String, String>,
}

fn thing() -> Thing {
    Thing {
        id: 1,
        extra: BTreeMap::from([("k".to_owned(), "v".to_owned())]),
    }
}

/// The declared members survive the flattened map.
///
/// The map's value schema describes the members the map contributes and says
/// nothing about `id`, which the parent declared and typed itself.
#[test]
fn a_flattened_map_leaves_the_parents_own_properties_alone() {
    let refusals = refusals(&thing());
    assert!(
        refusals.is_empty(),
        "the type cannot produce an instance its own description accepts: {refusals:?}\n\
         schema: {}",
        emitted::<Thing>()
    );
}

/// The map's values are still described.
///
/// The opposite failure to the one above, and the reason the repair cannot be
/// to drop the flattened schema: a member the map contributed must still be
/// held to the map's value type.
#[test]
fn a_flattened_map_still_constrains_the_members_it_contributes() {
    let schema = emitted::<Thing>();
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");

    assert!(
        !validator.is_valid(&serde_json::json!({ "id": 1, "k": 2 })),
        "a member contributed by a `BTreeMap<String, String>` was accepted as a number: {schema}"
    );
}
