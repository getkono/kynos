//! `#[schema(open)]` is the opt-in that keeps a flattened map, by saying the
//! object really is open and describing it with `unevaluatedProperties`.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: BTreeMap<String, String>,
}

fn main() {}
