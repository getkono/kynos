//! A flattened field whose type is a parameter declared only `Schema`: nothing
//! says what the parameter resolves to names its members, so the witness refuses
//! it at the declaration. The control is `pass/flatten_generic_field.rs`, which
//! declares the parameter `Flatten`.

#[derive(kynos::Schema, serde::Serialize)]
struct Audit {
    at: String,
}

#[derive(kynos::Schema, serde::Serialize)]
struct Page<T: kynos::schema::Schema> {
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
