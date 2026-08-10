fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<kynos::schema::unchecked::Unchecked<serde_json::Value>>();
}
