use std::collections::HashMap;

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<HashMap<String, String>>();
}
