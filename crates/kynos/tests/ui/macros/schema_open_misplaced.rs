//! `#[schema(open)]` supplies the `unevaluatedProperties` of the object a
//! flattened field is written into, so it is refused on a field that is not
//! flattened, which contributes no members, and on a second flattened field of
//! one object, which could only overwrite the first.
//!
//! The control is `pass/flatten_open_map.rs`: one flattened open field.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize)]
struct Unflattened {
    #[schema(open)]
    extra: BTreeMap<String, String>,
}

#[derive(kynos::Schema, serde::Serialize)]
struct OpenTwice {
    #[serde(flatten)]
    #[schema(open)]
    extra: BTreeMap<String, String>,
    #[serde(flatten)]
    #[schema(open)]
    more: BTreeMap<String, String>,
}

fn main() {}
