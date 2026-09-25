//! A struct holding a flattened field of its own, flattened into an object
//! serde reads under `deny_unknown_fields`: serde reads that struct as a map
//! and lends it the parent's keys without taking them, so the parent refuses
//! every member of every document the type writes. The control is
//! `pass/schema_closed_beside_flattened_struct_skipping_its_flatten`, whose
//! inner flattened field serde skips.

#[derive(Default, kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Audit {
    at: String,
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Extra {
    note: String,
    #[serde(flatten)]
    audit: Audit,
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    extra: Extra,
}

fn main() {}
