//! The control for `macros/schema_skip_serializing_without_default`: the same
//! field, differing only in that `default` lets serde read it absent too.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Draft {
    plain: u64,
    #[serde(default, skip_serializing)]
    elided: u64,
}

fn main() {}
