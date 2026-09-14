//! A struct names its members, so flattening one describes the object it writes.
//! The control for `macros/schema_flatten_transparent.rs`.

#[derive(kynos::Schema, serde::Serialize)]
struct Audit {
    at: String,
}

#[derive(kynos::Schema, serde::Serialize)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    audit: Audit,
}

fn main() {}
