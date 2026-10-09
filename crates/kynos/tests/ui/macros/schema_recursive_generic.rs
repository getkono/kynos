//! A generic type naming itself in a field its schema describes: a generic
//! type has no component name and is inlined, so nothing can stand in for its
//! body while it is being built, and the description would never end.

#[derive(kynos::Schema)]
struct Node<T> {
    value: T,
    next: Option<Box<Node<T>>>,
}

fn main() {}
