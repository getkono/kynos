//! A `#[serde(transparent)]` struct writes its one field's value rather than an
//! object naming the fields it declares, so flattening one over a map is
//! flattening the map. The control is `pass/flatten_named_struct.rs`, whose
//! struct is not transparent.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize)]
#[serde(transparent)]
struct Labels {
    labels: BTreeMap<String, String>,
}

#[derive(kynos::Schema, serde::Serialize)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    labels: Labels,
}

fn main() {}
