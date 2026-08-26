fn is_key<T: kynos::schema::MapKey>() {}

fn main() {
    is_key::<String>();
}
