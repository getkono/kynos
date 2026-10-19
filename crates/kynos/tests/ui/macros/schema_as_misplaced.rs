//! `#[schema(as = T)]` describes a field as one value of `T`, so it is written
//! once per field, and refused where serde writes no one value: a flattened
//! field and an internally tagged newtype variant's payload, whose members
//! serde writes into the object around them, and a `PhantomData`, which serde
//! writes as `null` whatever it is described as.
//!
//! The control is `pass/schema_as_with_conversion.rs`: one `as` on a field
//! serde writes as one value.

use std::marker::PhantomData;

struct Hash([u8; 2]);

impl From<&Hash> for String {
    fn from(hash: &Hash) -> Self {
        format!("{:02x}{:02x}", hash.0[0], hash.0[1])
    }
}

#[derive(kynos::Schema)]
struct Digest {
    #[schema(as = String)]
    #[schema(min_length = 1, as = String)]
    hash: Hash,
}

#[derive(kynos::Schema, serde::Serialize)]
struct Line {
    sku: String,
}

#[derive(kynos::Schema, serde::Serialize)]
struct Order {
    #[serde(flatten)]
    #[schema(as = Line)]
    line: Line,
}

#[derive(kynos::Schema, serde::Serialize)]
#[serde(tag = "kind")]
enum Event {
    Created(#[schema(as = Line)] Line),
}

#[derive(kynos::Schema)]
struct Marker<T> {
    #[schema(as = String)]
    marker: PhantomData<T>,
}

fn main() {}
