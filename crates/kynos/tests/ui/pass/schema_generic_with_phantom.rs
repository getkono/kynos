use std::marker::PhantomData;

// Bounding the type *parameter* rather than each field type is what keeps a
// field no schema describes from demanding one. A field-type bound would
// require `PhantomData<T>: Schema`, which nothing satisfies.
#[derive(kynos::Schema)]
struct Page<T> {
    items: Vec<T>,
    marker: PhantomData<T>,
}

#[derive(kynos::Schema)]
struct User {
    id: u64,
}

fn describable<T: kynos::schema::Schema>() {}

fn main() {
    describable::<Page<User>>();
    describable::<Vec<Page<User>>>();
}
