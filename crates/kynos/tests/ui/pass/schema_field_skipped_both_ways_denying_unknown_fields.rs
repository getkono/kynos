//! The control for `macros/schema_field_skipped_on_read_denying_unknown_fields`:
//! the same field, differing only in that `skip` keeps it out of what serde
//! writes too.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Thing {
    id: u64,
    #[serde(skip)]
    stamp: u64,
}

fn main() {}
