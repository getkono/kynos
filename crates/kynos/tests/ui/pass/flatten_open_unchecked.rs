//! The control for `macros/schema_flatten_unchecked.rs`: the same field with
//! `#[schema(open)]`, which is how arbitrary JSON is flattened.

#[derive(kynos::Schema, serde::Serialize)]
struct Envelope {
    id: u64,
    #[serde(flatten)]
    #[schema(open)]
    rest: kynos::schema::unchecked::Unchecked<serde_json::Map<String, serde_json::Value>>,
}

fn main() {}
