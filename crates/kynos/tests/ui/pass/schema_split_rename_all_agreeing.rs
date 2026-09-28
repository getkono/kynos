//! The control for `macros/schema_split_rename_all`: the same struct, differing
//! only in that both sides of the split `rename_all` name one style, which the
//! derive reads as `rename_all = "camelCase"`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "camelCase"))]
struct Stamp {
    created_at: u64,
}

fn main() {}
