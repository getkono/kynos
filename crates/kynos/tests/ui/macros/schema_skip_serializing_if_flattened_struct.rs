//! `skip_serializing_if` on a flattened struct, even beside a `default`: serde
//! ignores `default` on a flattened field, so the struct's required members are
//! still required on read while serde may leave them out of what it writes.
//! The control is `pass/schema_flattened_struct_written_whole`.

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
    #[serde(flatten, default, skip_serializing_if = "Audit::is_empty")]
    audit: Audit,
}

fn main() {}
