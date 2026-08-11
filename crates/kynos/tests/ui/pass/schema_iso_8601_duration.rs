//! The control for `schema/chrono_duration.rs`.
//!
//! A duration is describable when it writes an ISO 8601 string, so what the
//! negative proves is that the refusal is about the wire form rather than about
//! durations.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<jiff::Span>();
    describable::<jiff::SignedDuration>();
}
