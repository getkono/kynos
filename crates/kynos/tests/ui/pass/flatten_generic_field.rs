//! A flattened field whose type is a parameter. The bound is the one the type's
//! own declaration writes: the derive carries the input's generics into the
//! witness, so `T: Flatten` there is what satisfies it. The control for
//! `macros/schema_flatten_generic_field_unbounded.rs`.

use kynos::schema::Flatten;

#[derive(kynos::Schema, serde::Serialize)]
struct Audit {
    at: String,
}

#[derive(kynos::Schema, serde::Serialize)]
struct Page<T: Flatten> {
    total: u64,
    #[serde(flatten)]
    inner: T,
}

fn main() {
    let _ = Page {
        total: 1,
        inner: Audit { at: "x".to_owned() },
    };
}
