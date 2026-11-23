//! A `Nullable` member with no `#[serde(default)]`: serde reads it left out
//! as `null`, so a patch leaving it alone would clear it. The control is
//! `pass/schema_nullable_with_default`.

use kynos::schema::Nullable;

#[derive(kynos::Schema, serde::Deserialize)]
struct EntryPatch {
    title: Nullable<String>,
}

fn main() {}
