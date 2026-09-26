//! A field named as its struct's own `#[serde(tag = "...")]`: serde writes the
//! key twice and reads the tag's value back as the field. The control is
//! `pass/schema_tagged_struct_field_skipped_both_ways_named_as_tag`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
struct Stamp {
    at: u64,
    kind: String,
}

fn main() {}
