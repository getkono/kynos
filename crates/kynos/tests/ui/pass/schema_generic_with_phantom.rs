use std::marker::PhantomData;

// Bounding the type *parameter* rather than each field type is what keeps a
// marker field from demanding a schema of its own. A field-type bound would
// require `PhantomData<T>: Schema`, which nothing satisfies; the derive
// describes the marker as the `null` serde writes instead, named or positional.
#[derive(kynos::Schema)]
struct Page<T> {
    items: Vec<T>,
    marker: PhantomData<T>,
}

#[derive(kynos::Schema)]
struct Cursor<T>(u64, PhantomData<T>);

#[derive(kynos::Schema)]
struct User {
    id: u64,
}

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Page<User>>();
    describable::<Vec<Page<User>>>();
    describable::<Cursor<User>>();
}
