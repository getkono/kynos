use serde_json::json;

use crate::model::{
    document::{Document, SpecVersion},
    info::Info,
    paths::{item::PathItem, template::PathTemplate},
};

fn document() -> Document {
    Document::new(SpecVersion::V3_1, Info::new("Orders", "1.0.0"))
}

fn with_paths(templates: &[&str]) -> Document {
    let mut document = document();
    for template in templates {
        let template = PathTemplate::parse(*template).expect("a valid template");
        document.paths.insert(&template, PathItem::new());
    }
    document
}

/// The whole format, byte for byte: keys sorted by code point at every
/// depth, free-form values included, arrays in their own order, a two-space
/// indent and one trailing newline.
///
/// The extension values are built in an order the sort reverses, so under
/// `serde_json/preserve_order`, where a `Value` keeps insertion order, only the
/// writer's own sort passes; `mise run test:preserve-order` runs it there.
#[test]
fn canonical_json_sorts_every_object_and_no_array() {
    let mut document = document();
    document.extensions.insert(
        "x-order",
        json!({ "é": 1, "~": 2, "a": [3, 1, 2], "Z": {} }),
    );
    document.extensions.insert("x-first", json!([]));

    assert_eq!(
        document.to_canonical_json().expect("serializable"),
        r#"{
  "info": {
    "title": "Orders",
    "version": "1.0.0"
  },
  "openapi": "3.1.2",
  "paths": {},
  "x-first": [],
  "x-order": {
    "Z": {},
    "a": [
      3,
      1,
      2
    ],
    "~": 2,
    "é": 1
  }
}
"#
    );
}

/// Two documents differing only in insertion order have one canonical form,
/// though `to_json` writes each in its own order.
#[test]
fn insertion_order_does_not_reach_the_canonical_form() {
    let mounted = with_paths(&["/users", "/orders"]);
    let reordered = with_paths(&["/orders", "/users"]);

    assert_ne!(mounted.to_json().ok(), reordered.to_json().ok());
    assert_eq!(
        mounted.to_canonical_json().expect("serializable"),
        reordered.to_canonical_json().expect("serializable")
    );
}

/// An extension named like a field of its object makes one object carry a
/// key twice, which has no canonical form; refused, naming the key, rather
/// than keeping one of the two values.
#[test]
fn a_key_written_twice_is_refused_by_name() {
    let mut document = document();
    document.extensions.insert("info", json!(1));

    let error = document
        .to_canonical_json()
        .expect_err("`info` is written twice");
    assert!(
        error.to_string().contains("the key `info` appears twice"),
        "{error}"
    );
}
