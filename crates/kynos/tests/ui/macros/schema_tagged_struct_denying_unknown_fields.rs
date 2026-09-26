//! `#[serde(tag = "...")]` on a struct `deny_unknown_fields` closes: serde
//! writes the tag beside the fields and refuses it on read as an unknown field,
//! so no schema is true of both. The control is `pass/schema_tagged_struct`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
struct Stamp {
    at: u64,
}

fn main() {}
