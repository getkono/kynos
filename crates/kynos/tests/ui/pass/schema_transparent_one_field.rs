//! The control for `macros/schema_transparent_without_one_field`: the same
//! struct, differing only in which field serde writes through. `a` is skipped
//! both ways and `b` is not, so serde writes and reads through `b` under both
//! derives, and `Schema` describes it.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Split {
    #[serde(skip)]
    a: u64,
    b: String,
}

fn main() {}
