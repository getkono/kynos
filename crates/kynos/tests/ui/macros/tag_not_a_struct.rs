//! A tag is a name for a group of operations, declared as a unit struct, so an
//! enum is refused.
//!
//! The control is `pass/tag_without_fields.rs`.

#[derive(kynos::Tag)]
enum Users {
    A,
}

fn main() {}
