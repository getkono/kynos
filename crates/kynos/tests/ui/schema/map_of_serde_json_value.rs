//! `HashMap<String, Value>`: `additionalProperties: true`.

use std::collections::HashMap;

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<HashMap<String, serde_json::Value>>();
}
