//! A struct names its members even when its one member is a map, so flattening
//! it describes the object it writes. The control for
//! `macros/schema_flatten_transparent.rs`, which differs only in
//! `#[serde(transparent)]`.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize)]
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
