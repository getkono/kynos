//! `skip_deserializing` alone on a field of an object `deny_unknown_fields`
//! closes: the schema leaves out the field serde never reads, and the closed
//! object then refuses what serde writes of it. The control is
//! `pass/schema_field_skipped_both_ways_denying_unknown_fields`.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Thing {
    id: u64,
    #[serde(skip_deserializing)]
    stamp: u64,
}

fn main() {}
