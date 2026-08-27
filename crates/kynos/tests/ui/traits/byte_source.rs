//! Octets already in hand are not a byte *source*: a source is read from
//! asynchronously and in parts, which is what lets the whole representation
//! never exist in memory at once.

use kynos::{
    extract::{body::binary::Binary, media::OctetStream},
    response::range::source::ByteSource,
};

fn is_a_source<S: ByteSource>() {}

fn main() {
    is_a_source::<Binary<OctetStream>>();
}
