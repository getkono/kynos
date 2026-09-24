//! An open flattened map in an object serde reads under
//! `deny_unknown_fields`: serde refuses every key the fields do not name before
//! the map sees it, so it reads the map empty and writes members it would
//! refuse to read back. The control is
//! `pass/schema_closed_beside_flattened_struct`.

use std::collections::BTreeMap;

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: BTreeMap<String, String>,
}

fn main() {}
