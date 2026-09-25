//! What a name serde reads under an `alias` means to a validator.
//!
//! `derives.rs` fixes each aliased shape as exact JSON, which is transcribed
//! from the emitter and agrees with it wherever both are wrong. Here the
//! emitted schema is compiled by a real JSON Schema validator and every
//! document is held to serde's own read, so a bound that says something other
//! than what serde does fails even when the transcription moved with it.
//!
//! Only the bounds no other validator test reaches are here. A flattened
//! struct's alias, required and optional, is checked in `flatten.rs`.

// `test-util` carries the JSON Schema validator.
#![cfg(all(feature = "macros", feature = "test-util"))]

use kynos::{Schema, schema::Schema as SchemaTrait};
use serde::de::DeserializeOwned;

/// Asserts that the schema `T` emits admits exactly the documents serde reads
/// as a `T`, over `read` and `refused`.
fn held_to_serde<T: SchemaTrait + DeserializeOwned>(
    read: &[serde_json::Value],
    refused: &[serde_json::Value],
) {
    let mut registry = kynos::schema::registry::Registry::new();
    let schema = serde_json::to_value(T::schema(&mut registry)).expect("a schema serializes");
    let validator =
        jsonschema::draft202012::new(&schema).expect("an emitted schema compiles as draft 2020-12");

    for document in read {
        assert!(
            serde_json::from_value::<T>(document.clone()).is_ok(),
            "serde must read {document}"
        );
        assert!(
            validator.is_valid(document),
            "a document serde reads was refused: {document}\nschema: {schema}"
        );
    }
    for document in refused {
        assert!(
            serde_json::from_value::<T>(document.clone()).is_err(),
            "serde must refuse {document}"
        );
        assert!(
            !validator.is_valid(document),
            "a document serde refuses was accepted: {document}\nschema: {schema}"
        );
    }
}

/// A closed object with two optional aliased fields, one under two names and
/// one under three.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Reading {
    id: u64,
    #[serde(alias = "bee")]
    b: Option<u64>,
    #[serde(alias = "sea", alias = "see")]
    c: Option<u64>,
}

/// Three names are bounded by a `not` over an `anyOf` of their pairs, since
/// all three present match every pair, which a `oneOf` of the pairs would
/// admit. Each field's bound is an entry of its own in the object's `allOf`,
/// so the second field's leaves the first's in place. The closed object admits
/// each field under any one name or none.
#[test]
fn an_optional_field_under_three_names_is_present_under_at_most_one() {
    use serde_json::json;
    held_to_serde::<Reading>(
        &[
            json!({ "id": 1 }),
            json!({ "id": 1, "bee": 1, "c": 2 }),
            json!({ "id": 1, "sea": 2 }),
            json!({ "id": 1, "b": 1, "see": 2 }),
        ],
        &[
            json!({ "id": 1, "b": 1, "bee": 2 }),
            json!({ "id": 1, "c": 2, "sea": 3 }),
            json!({ "id": 1, "c": 2, "see": 3 }),
            json!({ "id": 1, "sea": 2, "see": 3 }),
            json!({ "id": 1, "c": 2, "sea": 3, "see": 4 }),
        ],
    );
}

/// An externally tagged enum whose object branch serde keys by an alias too.
#[derive(Schema, serde::Serialize, serde::Deserialize)]
enum Directive {
    #[serde(alias = "go")]
    Move {
        x: u64,
    },
    Say(String),
}

/// An externally tagged branch is one entry under one of its names, never
/// none, and closed to every other key. `Say` is the branch under one name,
/// listed in `required`, and the only branch besides `Move`'s, so `{}` is
/// refused only while `Move`'s bound refuses it too.
#[test]
fn an_externally_tagged_branch_is_keyed_by_exactly_one_of_its_names() {
    use serde_json::json;
    held_to_serde::<Directive>(
        &[json!({ "Move": { "x": 1 } }), json!({ "go": { "x": 1 } })],
        &[json!({}), json!({ "go": { "x": 1 }, "z": 2 })],
    );
}

// serde's derive matches a shared name twice, and the second arm is the
// unreachable pattern this module allows: serde reads it as the first variant.
#[allow(unreachable_patterns)]
mod overlapping {
    use kynos::Schema;

    /// An internally tagged enum whose later variant's alias is an earlier
    /// variant's tag value.
    #[derive(Schema, serde::Serialize, serde::Deserialize)]
    #[serde(tag = "kind")]
    pub(super) enum Signal {
        Start,
        #[serde(alias = "Start", alias = "halt")]
        Stop,
    }

    /// An externally tagged enum whose later variant's alias is an earlier
    /// variant's key.
    #[derive(Schema, serde::Serialize, serde::Deserialize)]
    pub(super) enum Command {
        Push(u64),
        #[serde(alias = "Push", alias = "tug")]
        Pull(u64),
    }
}

/// A name two variants claim is read as the first, so only the first's branch
/// names it: naming it in both makes it match two `oneOf` branches, which the
/// validator refuses though serde reads it. The later variant keeps the names
/// no earlier one claims.
#[test]
fn a_name_an_earlier_variant_claims_is_read_through_that_variant_alone() {
    use serde_json::json;
    held_to_serde::<overlapping::Signal>(
        &[json!({ "kind": "Start" }), json!({ "kind": "halt" })],
        &[],
    );
    held_to_serde::<overlapping::Command>(&[json!({ "Push": 1 }), json!({ "tug": 1 })], &[]);
}
