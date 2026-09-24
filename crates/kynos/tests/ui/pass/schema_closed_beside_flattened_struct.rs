//! A flattened struct in an object serde reads under `deny_unknown_fields`
//! expands: serde reads the keys the struct names, and the closed object admits
//! them across the `allOf`. The control for
//! `macros/schema_open_map_denying_unknown_fields`, whose flattened field is an
//! open map instead.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Extra {
    note: String,
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    extra: Extra,
}

fn main() {}
