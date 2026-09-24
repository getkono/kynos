//! The control for `macros/schema_field_skipped_on_read_beside_open_map.rs`:
//! the same field beside an open `Unchecked` payload, which hoists no
//! `unevaluatedProperties` to refuse it with.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Thing {
    id: u64,
    #[serde(skip_deserializing)]
    stamp: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: kynos::schema::unchecked::Unchecked<serde_json::Map<String, serde_json::Value>>,
}

fn main() {}
