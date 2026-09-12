//! The control for `ui/traits/flatten.rs`: the same bound, over a type whose
//! schema does name its members.

fn is_flattenable<T: kynos::schema::Flatten>() {}

#[derive(kynos::Schema, serde::Serialize)]
struct Audit {
    at: String,
}

fn main() {
    is_flattenable::<Audit>();
    // And through the wrappers that carry the answer across.
    is_flattenable::<Box<Audit>>();
    is_flattenable::<std::sync::Arc<Audit>>();
}
