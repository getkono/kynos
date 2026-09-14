//! The control for `macros/schema_skip_serializing_if_flattened_struct`: the
//! same flattened struct, differing only in that serde always writes it.

#[derive(Default, kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Audit {
    tags: Vec<String>,
}

impl Audit {
    fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Wrapper {
    id: u64,
    #[serde(flatten, default)]
    audit: Audit,
}

fn main() {}
