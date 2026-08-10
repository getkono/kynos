//! `serde_json::Value`, `Map` and `RawValue`: the schema would be `true`.

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<serde_json::Value>();
}
