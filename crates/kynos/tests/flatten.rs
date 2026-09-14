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
//!
//! A flattened map is the shape that does it, because a map names no member at
//! all. `kynos::schema::Flatten` is what refuses one outright; `#[schema(open)]`
//! is the declaration that the object really is open, and the case below is what
//! holds the description it then emits to the JSON the type actually writes.

// `test-util` carries the JSON Schema validator, which is what makes this a
// check against an oracle rather than an assertion about the emitter written
// from the emitter.
#![cfg(all(feature = "macros", feature = "test-util"))]

use std::collections::BTreeMap;

use kynos::{
    Schema,
    openapi::{
        Schema as OpenApiSchema, SchemaObject,
        model::schema::types::{SchemaType, TypeSet},
    },
    schema::{
        MapKey, OpenMap, Schema as SchemaTrait, constraints::Constraints, registry::Registry,
    },
};
use serde::Serialize;

/// The schema `T` emits, with everything it refers to reachable from the root.
///
/// A flattened *named* type resolves to a `$ref` into `#/components/schemas`,
/// so the components the registry collected travel beside the body or the
/// validator has nothing to follow the reference to.
fn emitted<T: SchemaTrait>() -> serde_json::Value {
    let mut registry = kynos::schema::registry::Registry::new();
    let body = T::schema(&mut registry);

    let mut root = serde_json::to_value(body).expect("a schema serializes");
    let components =
        serde_json::to_value(registry.into_components()).expect("the components serialize");
    root.as_object_mut()
        .expect("a derived schema is an object")
        .insert("components".to_owned(), components);
    root
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

/// A type that names its members, which is what makes it flattenable.
#[derive(Schema, Serialize)]
struct Audit {
    at: String,
}

/// The closed case: a flattened struct, composed through a `$ref`.
#[derive(Schema, Serialize)]
struct Audited {
    id: u64,
    #[serde(flatten)]
    audit: Audit,
}

/// The open case: a flattened map, which names no member and says so.
#[derive(Schema, Serialize)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: BTreeMap<String, String>,
}

fn audited() -> Audited {
    Audited {
        id: 1,
        audit: Audit {
            at: "2026-01-01T00:00:00Z".to_owned(),
        },
    }
}

fn thing() -> Thing {
    Thing {
        id: 1,
        extra: BTreeMap::from([("k".to_owned(), "v".to_owned())]),
    }
}

/// A flattened struct describes the object the type writes.
///
/// The asymmetry that makes the map case hard: a named type resolves to a
/// `$ref`, whose target carries `properties` of its own and constrains nothing
/// it does not name, so composing it with `allOf` is already correct.
#[test]
fn a_flattened_struct_leaves_the_parents_own_properties_alone() {
    let refusals = refusals(&audited());
    assert!(
        refusals.is_empty(),
        "the type cannot produce an instance its own description accepts: {refusals:?}\n\
         schema: {}",
        emitted::<Audited>()
    );
}

/// An open flattened map describes the object the type writes.
///
/// The declared `id` is a member the parent named, so the map's value schema
/// must not reach it. `unevaluatedProperties` is the keyword that says so: it
/// sees the `properties` annotation across the `allOf`, which
/// `additionalProperties` does not.
#[test]
fn an_open_flattened_map_leaves_the_parents_own_properties_alone() {
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
/// The opposite failure, and the reason the repair cannot be to drop the
/// flattened schema: a member the map contributed must still be held to the
/// map's value type.
#[test]
fn an_open_flattened_map_still_constrains_the_members_it_contributes() {
    let schema = emitted::<Thing>();
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");

    assert!(
        !validator.is_valid(&serde_json::json!({ "id": 1, "k": 2 })),
        "a member contributed by a `BTreeMap<String, String>` was accepted as a number: {schema}"
    );
}

/// The keyword the open case emits, named rather than only exercised.
///
/// `unevaluatedProperties` and not `additionalProperties`: the second is
/// defined against its own schema object's `properties`, so hoisting the map's
/// value schema there would be correct for a lone flattened map and wrong the
/// moment a second flattened field contributed properties through a `$ref`.
#[test]
fn an_open_flattened_map_hoists_its_values_to_unevaluated_properties() {
    let schema = emitted::<Thing>();

    assert_eq!(
        schema["unevaluatedProperties"],
        serde_json::json!({ "type": "string" }),
        "{schema}"
    );
    assert!(
        schema["allOf"][0]["additionalProperties"].is_null(),
        "the map's `additionalProperties` stayed inside the `allOf` branch: {schema}"
    );
    assert!(
        schema["additionalProperties"].is_null(),
        "the map's values reached the parent's `additionalProperties`: {schema}"
    );
}

/// Both compositions on one container, which is the case the keyword was
/// chosen for.
///
/// `Audit` contributes `at` through a `$ref`, and the map contributes whatever
/// is left. `additionalProperties` hoisted to the parent would see only the
/// parent's own `properties` and so refuse `at`; `unevaluatedProperties` also
/// sees the `properties` annotation the `$ref` produced. That difference is the
/// whole reason the second keyword was chosen, and until this case existed
/// nothing exercised it.
#[derive(Schema, Serialize)]
struct Both {
    id: u64,
    #[serde(flatten)]
    audit: Audit,
    #[serde(flatten)]
    #[schema(open)]
    extra: BTreeMap<String, String>,
}

fn both() -> Both {
    Both {
        id: 1,
        audit: Audit {
            at: "2026-01-01T00:00:00Z".to_owned(),
        },
        extra: BTreeMap::from([("k".to_owned(), "v".to_owned())]),
    }
}

/// A closed flattened struct and an open flattened map compose.
#[test]
fn an_open_map_beside_a_flattened_struct_accepts_what_each_contributes() {
    let refusals = refusals(&both());
    assert!(
        refusals.is_empty(),
        "the type cannot produce an instance its own description accepts: {refusals:?}\n\
         schema: {}",
        emitted::<Both>()
    );
}

/// And composing them weakens neither.
///
/// The converse of the case above. A description that merely admitted every
/// member would pass that one, so this separates them from both sides: a member
/// the map contributed still answers to the map's value schema, and the
/// flattened struct's own property is still required.
#[test]
fn an_open_map_beside_a_flattened_struct_weakens_neither() {
    let schema = emitted::<Both>();
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");

    assert!(
        !validator.is_valid(&serde_json::json!({
            "id": 1, "at": "2026-01-01T00:00:00Z", "k": 2
        })),
        "a member contributed by a `BTreeMap<String, String>` was accepted as a number: {schema}"
    );
    assert!(
        !validator.is_valid(&serde_json::json!({ "id": 1, "k": "v" })),
        "the flattened struct's own required property was not required: {schema}"
    );
}

/// A map key carrying a constraint, which an open flatten has nowhere to put.
#[derive(Schema, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct Sku(String);

impl MapKey for Sku {
    fn key_constraints() -> Constraints {
        let mut constraints = Constraints::default();
        constraints.pattern = Some("^[A-Z]{3}-[0-9]{4}$".to_owned());
        constraints
    }
}

/// The same map as a property, where its key constraint has a schema of its own.
#[derive(Schema, Serialize)]
struct Ledger {
    counts: BTreeMap<Sku, u64>,
}

/// And flattened open, where it does not.
#[derive(Schema, Serialize)]
struct Stock {
    id: u64,
    #[serde(flatten)]
    #[schema(open)]
    counts: BTreeMap<Sku, u64>,
}

/// An open flatten drops a key constraint rather than moving it.
///
/// Inside the `allOf` branch `propertyNames` would name `id` as well, and `id`
/// matches no SKU pattern, so keeping it would refuse the JSON the type writes.
/// The description is left weaker than `Sku` instead. `Ledger` is the same map
/// as a property, which is what makes the absence here an observation: there
/// the constraint is emitted.
#[test]
fn an_open_flatten_drops_the_key_constraint_rather_than_reaching_the_parent() {
    assert!(
        emitted::<Ledger>().to_string().contains("propertyNames"),
        "a map keyed by `Sku` emitted no key constraint at all: {}",
        emitted::<Ledger>()
    );

    let stock = Stock {
        id: 1,
        counts: BTreeMap::from([(Sku("ABC-0001".to_owned()), 3)]),
    };
    let schema = emitted::<Stock>();
    assert!(
        !schema.to_string().contains("propertyNames"),
        "a key constraint survived an open flatten: {schema}"
    );
    let refusals = refusals(&stock);
    assert!(
        refusals.is_empty(),
        "the type cannot produce an instance its own description accepts: {refusals:?}\n\
         schema: {schema}"
    );
}

/// An open map inside an internally tagged variant.
///
/// The variant's object carries the tag beside its own properties, and the
/// hoisted `unevaluatedProperties` sits on that object, so both the tag and
/// `id` are evaluated before the map's value schema applies.
#[derive(Schema, Serialize)]
#[serde(tag = "kind")]
enum Tally {
    Counted {
        id: u64,
        #[serde(flatten)]
        #[schema(open)]
        extra: BTreeMap<String, u64>,
    },
}

#[test]
fn an_open_map_in_a_tagged_variant_leaves_the_tag_and_the_variants_properties_alone() {
    let tally = Tally::Counted {
        id: 1,
        extra: BTreeMap::from([("k".to_owned(), 2)]),
    };
    let refusals = refusals(&tally);
    assert!(
        refusals.is_empty(),
        "the type cannot produce an instance its own description accepts: {refusals:?}\n\
         schema: {}",
        emitted::<Tally>()
    );

    let schema = emitted::<Tally>();
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");
    assert!(
        !validator.is_valid(&serde_json::json!({ "kind": "Counted", "id": 1, "k": "v" })),
        "a member contributed by a `BTreeMap<String, u64>` was accepted as a string: {schema}"
    );
}

/// The same open map inside an adjacently tagged variant.
///
/// The variant's fields sit under the content key rather than beside the tag,
/// so the hoisted `unevaluatedProperties` belongs to the content object and has
/// `id` and `name` to leave alone. `name` is a string where the map's values are
/// numbers, so a value schema left inside the `allOf` would refuse it.
#[derive(Schema, Serialize)]
#[serde(tag = "kind", content = "data")]
enum Wrapped {
    Counted {
        id: u64,
        name: String,
        #[serde(flatten)]
        #[schema(open)]
        extra: BTreeMap<String, u64>,
    },
}

#[test]
fn an_open_map_in_an_adjacently_tagged_variant_leaves_the_variants_properties_alone() {
    let wrapped = Wrapped::Counted {
        id: 1,
        name: "n".to_owned(),
        extra: BTreeMap::from([("k".to_owned(), 2)]),
    };
    let refusals = refusals(&wrapped);
    assert!(
        refusals.is_empty(),
        "the type cannot produce an instance its own description accepts: {refusals:?}\n\
         schema: {}",
        emitted::<Wrapped>()
    );

    let schema = emitted::<Wrapped>();
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");
    assert!(
        !validator.is_valid(&serde_json::json!({
            "kind": "Counted", "data": { "id": 1, "name": "n", "k": "v" }
        })),
        "a member contributed by a `BTreeMap<String, u64>` was accepted as a string: {schema}"
    );
}

/// A map described by hand rather than by a standard map's own implementation.
///
/// `OpenMap` is unsealed so a type like this can claim what `HashMap` and
/// `BTreeMap` claim: no component name, and an object described by
/// `additionalProperties` alone. The hoist reads that schema object rather than
/// the type, so this holds it to the documented contract and not to the two maps.
#[derive(Serialize)]
struct Headers(BTreeMap<String, String>);

impl SchemaTrait for Headers {
    fn schema(_registry: &mut Registry) -> OpenApiSchema {
        OpenApiSchema::Object(Box::new(SchemaObject {
            ty: Some(TypeSet::One(SchemaType::Object)),
            additional_properties: Some(Box::new(OpenApiSchema::of_type(SchemaType::String))),
            ..SchemaObject::default()
        }))
    }
}

impl OpenMap for Headers {}

#[derive(Schema, Serialize)]
struct Request {
    id: u64,
    #[serde(flatten)]
    #[schema(open)]
    headers: Headers,
}

#[test]
fn a_hand_written_open_map_is_hoisted_like_a_standard_one() {
    assert!(<Headers as SchemaTrait>::name().is_none());

    let request = Request {
        id: 1,
        headers: Headers(BTreeMap::from([("k".to_owned(), "v".to_owned())])),
    };
    let refusals = refusals(&request);
    assert!(
        refusals.is_empty(),
        "the type cannot produce an instance its own description accepts: {refusals:?}\n\
         schema: {}",
        emitted::<Request>()
    );

    let schema = emitted::<Request>();
    assert_eq!(
        schema["unevaluatedProperties"],
        serde_json::json!({ "type": "string" }),
        "{schema}"
    );
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");
    assert!(
        !validator.is_valid(&serde_json::json!({ "id": 1, "k": 2 })),
        "a member contributed by `Headers` was accepted as a number: {schema}"
    );
}

/// A problem document carrying a member of its own type, the shape RFC 9457's
/// extension members exist for.
#[derive(Schema, Serialize)]
struct Extended {
    #[serde(flatten)]
    problem: kynos::Problem,
    balance: u32,
}

/// A flattened `Problem` describes the object the type writes.
///
/// `Problem` names its five registered members and admits the rest with
/// `additionalProperties: true`, which reaches the parent's `balance` from inside
/// the `allOf` and permits it. So the composition accepts both an extension the
/// problem carries and the member the parent declared, while the registered
/// members stay typed.
#[test]
fn a_flattened_problem_accepts_its_extension_members() {
    let extended = Extended {
        problem: kynos::Problem::new(kynos::http::StatusCode::FORBIDDEN)
            .with_detail("d")
            .with_extension("accounts", serde_json::json!(["/a"])),
        balance: 30,
    };
    assert_eq!(
        serde_json::to_value(&extended).expect("the value serializes"),
        serde_json::json!({
            "accounts": ["/a"], "balance": 30, "detail": "d",
            "status": 403, "title": "Forbidden", "type": "about:blank"
        })
    );
    let refusals = refusals(&extended);
    assert!(
        refusals.is_empty(),
        "the type cannot produce an instance its own description accepts: {refusals:?}\n\
         schema: {}",
        emitted::<Extended>()
    );

    let schema = emitted::<Extended>();
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");
    assert!(
        !validator.is_valid(&serde_json::json!({
            "type": "about:blank", "status": "403", "balance": 30
        })),
        "a problem's `status` was accepted as a string: {schema}"
    );
}
