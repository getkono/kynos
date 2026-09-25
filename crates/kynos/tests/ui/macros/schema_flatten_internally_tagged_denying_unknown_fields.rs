//! An internally tagged enum flattened into an object serde reads under
//! `deny_unknown_fields`: serde lends the enum the parent's keys without taking
//! them, so the parent refuses the tag of every document the type writes. The
//! control is `pass/schema_closed_beside_flattened_adjacently_tagged_enum`,
//! whose enum is adjacently tagged instead.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
enum Mode {
    Fixed { level: u8 },
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Thing {
    id: u64,
    #[serde(flatten)]
    mode: Mode,
}

fn main() {}
