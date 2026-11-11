//! The control for `schema/environment_dependent_offset.rs`.
//!
//! The same `DateTime`, differing only in the time zone parameter: an offset
//! the type names is describable, an offset the environment supplies is not.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<chrono04::DateTime<chrono04::Utc>>();
    describable::<chrono04::DateTime<chrono04::FixedOffset>>();
}
