//! The control for `macros/schema_tagged_newtype_generic.rs`: an internally
//! tagged newtype variant over a type parameter composes once the parameter is
//! declared `Flatten`, since the payload witness asks exactly that of it.

#[derive(kynos::Schema, serde::Serialize)]
struct Audit {
    at: String,
}

#[derive(kynos::Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Event<T: kynos::schema::flatten::Flatten> {
    Wrapped(T),
}

fn main() {
    let _: Option<Event<Audit>> = None;
}
