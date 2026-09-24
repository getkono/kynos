//! A field without `alias` in an object serde reads under
//! `deny_unknown_fields` expands: serde reads it under the one name the closed
//! object names. The control for `macros/schema_alias_denying_unknown_fields`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Thing {
    id: u64,
}

fn main() {}
