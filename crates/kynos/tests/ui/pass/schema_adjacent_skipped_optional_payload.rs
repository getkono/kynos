//! The control for `macros/schema_adjacent_skipped_payload`: the same variant,
//! differing only in that the skipped member is an `Option`, which serde reads
//! absent, so the tag alone it writes reads back.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t", content = "c")]
enum Reading {
    Count(u64),
    Hidden(#[serde(skip)] Option<u64>),
}

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Reading>();
}
