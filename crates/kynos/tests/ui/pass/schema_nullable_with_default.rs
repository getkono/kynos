//! The control for `macros/schema_nullable_without_default`: the same member,
//! differing only in that `default` fills it with `Nullable::Absent` when it is
//! left out.

use kynos::schema::Nullable;

#[derive(kynos::Schema, serde::Deserialize)]
struct EntryPatch {
    #[serde(default)]
    title: Nullable<String>,
}

fn main() {}
