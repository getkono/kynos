//! YAML emission, whatever `serde_json` features the build unifies.
//!
//! Cargo unifies features across a whole dependency graph, so a program can be
//! built with `serde_json/arbitrary_precision` on without asking for it. These
//! hold in the default graph too, but that is not where they can fail:
//! `mise run test:arbitrary-precision` runs them, with the rest of the crate,
//! under the graph that switch is on in, excluding by name the one test that
//! holds `to_yaml` to `serde_norway` byte for byte.

use serde_norway::{Number, Value};

use super::{
    YamlError,
    numbers::{number_from_digits, restore},
};
use crate::model::{
    document::{Document, SpecVersion},
    info::Info,
};

fn document() -> Document {
    Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"))
}

/// An extension key that repeats a model field is written as `serde_norway`
/// writes the model: beside the field, not in place of it.
///
/// A document carrying one is malformed, since an extension's name starts with
/// `x-`, but emitting it must not silently drop the field it collides with. A
/// route through an intermediate `serde_norway::Value` would, because a
/// mapping holds one value per key. Excluded by name from
/// `test:arbitrary-precision`, on purpose: under that task that route is the
/// one taken, and the two sides differ there by design.
#[test]
fn an_extension_repeating_a_model_field_leaves_the_field_in_place() {
    let mut document = document();
    document
        .extensions
        .0
        .insert("info".to_owned(), serde_json::json!(1));

    assert_eq!(
        document
            .to_yaml()
            .expect("the document is representable in YAML"),
        serde_norway::to_string(&document).expect("the model serializes to YAML")
    );
}

/// A JSON number reaches YAML as a number, at the top of a value and
/// nested inside one.
///
/// `x-wide` is beyond `u64`, which `serde_json` holds as a float without the
/// feature and as its digits with it; either way YAML gets the float.
/// `x-code` is the control: a string of digits stays a string.
#[test]
fn a_json_number_emits_as_a_yaml_number() {
    let mut document = document();
    for (key, value) in [
        ("x-limit", serde_json::json!(42)),
        ("x-offset", serde_json::json!(-7)),
        ("x-ratio", serde_json::json!(0.5)),
        (
            "x-wide",
            serde_json::from_str("18446744073709551616").expect("a JSON number"),
        ),
        ("x-nested", serde_json::json!({"tiers": [1, 2.5]})),
        ("x-code", serde_json::json!("42")),
    ] {
        document.extensions.0.insert(key.to_owned(), value);
    }

    let yaml = document.to_yaml().expect("every number here fits a float");
    let emitted: Value = serde_norway::from_str(&yaml).expect("emitted YAML parses");

    for (node, expected) in [
        (&emitted["x-limit"], Number::from(42u64)),
        (&emitted["x-offset"], Number::from(-7i64)),
        (&emitted["x-ratio"], Number::from(0.5)),
        (
            &emitted["x-wide"],
            Number::from(18_446_744_073_709_551_616.0),
        ),
        (&emitted["x-nested"]["tiers"][0], Number::from(1u64)),
        (&emitted["x-nested"]["tiers"][1], Number::from(2.5)),
    ] {
        assert_eq!(node, &Value::Number(expected), "in:\n{yaml}");
    }
    assert_eq!(
        emitted["x-code"],
        Value::String("42".to_owned()),
        "in:\n{yaml}"
    );
}

/// Each spelling of digits becomes the number `serde_json` holds for the
/// same text with `arbitrary_precision` off.
///
/// Compared through `Debug` too, because `Number`'s equality reads `-0.0`
/// and `0.0` as one value, and the sign is what the `-0` row is for.
#[test]
fn digits_become_the_number_serde_json_holds_without_arbitrary_precision() {
    for (digits, expected) in [
        ("42", Number::from(42u64)),
        ("18446744073709551615", Number::from(u64::MAX)),
        ("-7", Number::from(-7i64)),
        ("-9223372036854775808", Number::from(i64::MIN)),
        ("-0", Number::from(-0.0)),
        ("0.5", Number::from(0.5)),
        ("1e+140", Number::from(1e140)),
        (
            "18446744073709551616",
            Number::from(18_446_744_073_709_551_616.0),
        ),
    ] {
        let number = number_from_digits(digits).expect("a number a float can hold");
        assert_eq!(number, expected, "{digits}");
        assert_eq!(format!("{number:?}"), format!("{expected:?}"), "{digits}");
    }

    // Refused for two causes under one message: digits beyond any float,
    // and a token-shaped object built by hand whose string is no number.
    for digits in ["1e+400", "abc"] {
        let error = number_from_digits(digits).expect_err("no float holds it");
        assert_eq!(
            error.to_string(),
            format!("a number serde_json holds as `{digits}` cannot be emitted as a YAML number"),
        );
    }
}

/// `restore` rewrites a mapping only when it is exactly a token holding
/// digits, reaches one inside a tag, and lets a refused conversion out.
///
/// Over hand-built values, so it holds in both graphs: no document the
/// model builds puts a token under a tag, beside another key, or over a
/// non-string, and those are the arms a walk gets wrong without failing.
#[test]
fn restore_rewrites_exactly_a_token_mapping() {
    use serde_norway::{
        Mapping,
        value::{Tag, TaggedValue},
    };

    const TOKEN: &str = "$serde_json::private::Number";
    let token = |digits: Value| -> Value {
        Value::Mapping([(Value::from(TOKEN), digits)].into_iter().collect())
    };
    let tagged = |value: Value| -> Value {
        Value::Tagged(Box::new(TaggedValue {
            tag: Tag::new("Variant"),
            value,
        }))
    };

    let mut under_a_tag = tagged(token(Value::from("42")));
    restore(&mut under_a_tag).expect("42 is a number");
    assert_eq!(under_a_tag, tagged(Value::Number(Number::from(42u64))));

    for untouched in [
        Value::Mapping(
            [
                (Value::from(TOKEN), Value::from("42")),
                (Value::from("x-other"), Value::Null),
            ]
            .into_iter()
            .collect::<Mapping>(),
        ),
        token(Value::Number(Number::from(42u64))),
        // One key over one string, which is a token's shape and a
        // reference's too: only the key tells them apart.
        Value::Mapping(
            [(Value::from("$ref"), Value::from("#/components/schemas/Foo"))]
                .into_iter()
                .collect::<Mapping>(),
        ),
        Value::Bool(true),
    ] {
        let mut walked = untouched.clone();
        restore(&mut walked).expect("nothing here is converted");
        assert_eq!(walked, untouched);
    }

    let error = restore(&mut token(Value::from("1e+400"))).expect_err("beyond any float");
    assert_eq!(
        error.to_string(),
        "a number serde_json holds as `1e+400` cannot be emitted as a YAML number"
    );
}

/// `YamlError` says which encoding failed and keeps what the YAML library
/// said as its source, whichever way it was made.
#[test]
fn a_yaml_error_names_the_encoding_and_keeps_its_cause() {
    use std::error::Error as _;

    let refusal = "a number serde_json holds as `abc` cannot be emitted as a YAML number";
    for error in [
        YamlError(number_from_digits("abc").expect_err("no float holds it")),
        <YamlError as serde::ser::Error>::custom(refusal),
    ] {
        assert_eq!(
            error.to_string(),
            "the description could not be emitted as YAML"
        );
        assert_eq!(
            error.source().map(ToString::to_string).as_deref(),
            Some(refusal)
        );
    }
}
