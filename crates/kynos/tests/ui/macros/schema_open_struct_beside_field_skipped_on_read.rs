//! A named struct under `#[schema(open)]` beside a field serde never reads: it
//! is not an open map at all, so the refusal must not claim it hoists
//! anything. The control is
//! `pass/schema_field_skipped_on_read_beside_open_unchecked`, differing in the
//! open field's type.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Extra {
    note: String,
}

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
struct Thing {
    id: u64,
    #[serde(skip_deserializing)]
    stamp: u64,
    #[serde(flatten)]
    #[schema(open)]
    extra: Extra,
}

fn main() {}
