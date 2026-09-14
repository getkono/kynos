//! The control for `macros/schema_skip_serializing_if_flattened_map`: the same
//! flattened map, differing only in that it is declared `#[schema(open)]`.

use std::collections::HashMap;

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Tagged {
    id: u64,
    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    #[schema(open)]
    extra: HashMap<String, String>,
}

fn main() {}
