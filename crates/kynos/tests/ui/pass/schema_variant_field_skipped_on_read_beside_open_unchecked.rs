//! The control for `macros/schema_variant_field_skipped_on_read_beside_open_map`:
//! the same variant, differing only in an open field that hoists nothing.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
enum Event {
    Created {
        at: u64,
        #[serde(skip_deserializing)]
        stamp: u64,
        #[serde(flatten)]
        #[schema(open)]
        extra: kynos::schema::unchecked::Unchecked<serde_json::Map<String, serde_json::Value>>,
    },
}

fn main() {}
