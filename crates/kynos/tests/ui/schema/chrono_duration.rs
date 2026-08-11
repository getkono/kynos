//! `chrono::TimeDelta`: serde emits a `[seconds, nanos]` array, which is the
//! shape `std::time::Duration` is already refused for. The control is
//! `pass/schema_iso_8601_duration.rs`, which describes a duration that writes
//! ISO 8601 instead.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<chrono::TimeDelta>();
}
