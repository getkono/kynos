//! An adjacently tagged enum flattened into an object serde reads under
//! `deny_unknown_fields` expands: serde reads it through `deserialize_struct`,
//! which takes its tag and content keys. The control for
//! `macros/schema_flatten_internally_tagged_denying_unknown_fields`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value")]
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
