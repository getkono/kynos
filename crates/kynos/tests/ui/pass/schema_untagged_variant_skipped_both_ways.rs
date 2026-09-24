//! The control for `macros/schema_untagged_variant`: the same variant,
//! differing only in that `skip` leaves it out of what serde writes and reads,
//! so it is in no schema.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
enum Reading {
    Labelled { value: u64 },
    #[serde(skip)]
    #[serde(untagged)]
    Bare(u64),
}

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Reading>();
}
