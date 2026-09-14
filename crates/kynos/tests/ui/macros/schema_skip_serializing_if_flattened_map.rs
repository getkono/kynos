//! `skip_serializing_if` on a flattened map that is not `#[schema(open)]`. The
//! refusal reads attributes rather than types, so its one message names the
//! remedy for a map and for a struct alike. The control is
//! `pass/schema_skip_serializing_if_flattened_open_map`.

use std::collections::HashMap;

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Tagged {
    id: u64,
    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    extra: HashMap<String, String>,
}

fn main() {}
