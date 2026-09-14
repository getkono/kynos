//! The control for `macros/schema_transparent_without_one_field`: the same
//! struct, differing only in that `extra` is skipped, which leaves `value` the
//! one field serde reads and `Schema` describes.

#[derive(kynos::Schema, serde::Deserialize)]
#[serde(transparent)]
struct Reading {
    value: u64,
    #[serde(skip)]
    extra: String,
}

fn main() {}
