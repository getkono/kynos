//! The control for `macros/schema_field_skipped_on_read_beside_open_map`: the
//! same field, differing only in that `skip` keeps it out of what serde writes
//! too.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Thing {
    id: u64,
    #[serde(skip)]
    stamp: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: BTreeMap<String, String>,
}

fn main() {}
