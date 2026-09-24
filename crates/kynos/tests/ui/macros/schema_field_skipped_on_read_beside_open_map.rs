//! `skip_deserializing` alone on a field beside an open flattened map: the schema
//! leaves out the field serde never reads, and the map's `unevaluatedProperties`
//! then refuses what serde writes of it. The controls are
//! `pass/schema_field_skipped_both_ways_beside_open_map`, differing in the skip,
//! and `pass/schema_field_skipped_on_read_beside_open_unchecked`, differing in
//! an open field that hoists nothing.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Thing {
    id: u64,
    #[serde(skip_deserializing)]
    stamp: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: BTreeMap<String, String>,
}

fn main() {}
