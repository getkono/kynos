//! A streamed body is not rangeable: it states no complete length and cannot
//! seek, and RFC 9110 section 14.1.2 makes every byte offset relative to one.

use kynos::{
    extract::media::OctetStream,
    response::{range::rangeable::Rangeable, stream::binary::BinaryStream},
};

struct Chunks;

fn is_rangeable<T: Rangeable>() {}

fn main() {
    is_rangeable::<BinaryStream<Chunks, OctetStream>>();
}
