//! A JSON object key is a string, so a numeric key cannot describe one.

fn is_key<T: kynos::schema::MapKey>() {}

fn main() {
    is_key::<u32>();
}
