//! The control for `traits/byte_source.rs`: the shipped in-memory source is
//! one. Only the type differs.

use kynos::response::range::source::{ByteSource, InMemory};

fn is_a_source<S: ByteSource>() {}

fn main() {
    is_a_source::<InMemory>();
}
