//! The control for `macros/schema_tagged_newtype_map.rs`: a payload that names
//! its members composes beside the tag.

#[derive(kynos::Schema, serde::Serialize)]
struct Audit {
    at: String,
}

#[derive(kynos::Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Event {
    Audited(Audit),
}

fn main() {}
