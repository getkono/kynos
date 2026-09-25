//! A struct whose own flattened field serde skips, flattened into an object
//! serde reads under `deny_unknown_fields`, expands: with nothing of its own to
//! flatten, serde reads the struct through `deserialize_struct`, which takes the
//! keys it names. The control for
//! `macros/schema_flatten_nested_flatten_denying_unknown_fields`.

#[derive(Default, kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Audit {
    at: String,
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Extra {
    note: String,
    #[serde(flatten)]
    #[serde(skip)]
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
