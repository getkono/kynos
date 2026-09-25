//! `#[schema(open)]` hoists a map's values onto the object carrying it, so the
//! flattened type has to be a map the description writes out in place.

fn is_open_map<T: kynos::schema::flatten::OpenMap>() {}

fn main() {
    is_open_map::<String>();
}
