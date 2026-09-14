//! An internally tagged newtype variant over a type parameter composes whatever
//! the parameter resolves to beside the tag, so the parameter has to be declared
//! `Flatten`; `Schema` alone says nothing about its members.

#[derive(kynos::Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Event<T> {
    Wrapped(T),
}

fn main() {}
