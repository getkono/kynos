//! The control for `macros/schema_catch_all_variant`: the same enum, differing
//! only in that no variant catches the tags it does not name.

#[derive(kynos::Schema, serde::Deserialize)]
#[serde(tag = "kind")]
enum Event {
    Created { id: u64 },
    Deleted { id: u64 },
    Unknown,
}

fn main() {}
