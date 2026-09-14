//! A flattened field's members become the parent's own, so its schema has to
//! name them. A map names none.

fn is_flattenable<T: kynos::schema::Flatten>() {}

fn main() {
    is_flattenable::<std::collections::BTreeMap<String, String>>();
}
