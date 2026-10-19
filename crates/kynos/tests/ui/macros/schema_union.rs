//! A union holds one of its fields with nothing saying which, so no JSON value
//! corresponds to it and the derive refuses it.
//!
//! The control is any struct the derive describes, such as
//! `pass/schema_tagged_struct.rs`.

#[derive(kynos::Schema)]
union Payload {
    a: u32,
}

fn main() {}
