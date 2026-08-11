//! `chrono::DateTime<Local>`: the string is valid RFC 3339, but its offset
//! comes from the process environment, so the wire contract would depend on
//! where the server runs. The control is
//! `pass/schema_explicit_offset_instant.rs`, which differs only in the time
//! zone parameter.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<chrono::DateTime<chrono::Local>>();
}
