//! The control for `macros/schema_tagged_struct_field_named_as_tag`: the same
//! field, differing only in that `skip` keeps it out of what serde writes and
//! reads, so the tag is the only member under its name.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
struct Stamp {
    at: u64,
    #[serde(skip)]
    kind: String,
}

fn main() {}
