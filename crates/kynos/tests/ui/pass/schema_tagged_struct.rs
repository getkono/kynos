//! The control for `macros/schema_tagged_struct_denying_unknown_fields`: the
//! same struct, differing only in that it is open, so serde reads back the tag
//! it writes.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
struct Stamp {
    at: u64,
}

fn main() {}
