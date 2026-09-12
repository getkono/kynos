//! A struct names its members, so flattening one describes the object it writes.

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
