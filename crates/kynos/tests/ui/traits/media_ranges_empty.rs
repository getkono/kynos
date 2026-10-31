//! An empty `MediaRanges` set does not compile.
//!
//! It admits no type, so every delivery naming it would answer 500, and it
//! would describe a 200 with no `content` at all.

use bytes::Bytes;
use kynos::{
    http::media::MediaRanges,
    response::range::{served::Served, source::InMemory},
};

struct Nothing;

impl MediaRanges for Nothing {
    const RANGES: &'static [&'static str] = &[];
}

fn main() {
    let _ = Served::<_, Nothing>::new(InMemory::new(Bytes::new()));
}
