//! The control for `macros/schema_split_rename`: the same field, differing
//! only in that `skip_serializing` keeps it out of what serde writes, so it is
//! named by the side serde reads it under. `default` is what serde's missing
//! field then reads as.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Stamp {
    #[serde(skip_serializing, default, rename(serialize = "a", deserialize = "b"))]
    at: u64,
}

fn main() {}
