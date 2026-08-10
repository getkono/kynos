fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<i64>();
}
