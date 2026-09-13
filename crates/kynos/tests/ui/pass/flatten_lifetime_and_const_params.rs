//! The witness carries every kind of generic parameter the input declares, not
//! only types: a lifetime and a const parameter reach its `where` clause too,
//! or the `const _` block names parameters it never declared.

use std::marker::PhantomData;

#[derive(kynos::Schema, serde::Serialize)]
struct Audit {
    at: String,
}

#[derive(kynos::Schema, serde::Serialize)]
struct Window<'a, const N: usize> {
    id: u64,
    #[serde(flatten)]
    audit: Audit,
    #[serde(skip)]
    marker: PhantomData<&'a [u8; N]>,
}

fn main() {}
