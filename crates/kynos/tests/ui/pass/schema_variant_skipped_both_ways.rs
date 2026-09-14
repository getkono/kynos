//! The control for `macros/schema_variant_skipped_on_read`: the same variant,
//! differing only in that `skip_serializing` leaves it out of what serde writes
//! too.

#[derive(kynos::Schema, serde::Serialize, serde::Deserialize)]
enum Channel {
    Web,
    #[serde(skip_serializing)]
    #[serde(skip_deserializing)]
    Fax,
}

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Channel>();
}
