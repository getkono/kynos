//! `SystemTime`, `Instant` and `Duration`: serde emits a seconds/nanos struct.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<std::time::SystemTime>();
}
