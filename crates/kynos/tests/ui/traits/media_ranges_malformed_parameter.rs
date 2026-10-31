//! A `MediaRanges` entry whose parameter is malformed does not compile.
//!
//! RFC 9110 section 5.6.6 has `name=value`, and no sent type could carry a
//! parameter with no value, which would otherwise be described as a `content`
//! key that nothing matches.

use bytes::Bytes;
use kynos::{
    http::media::MediaRanges,
    response::range::{served::Served, source::InMemory},
};

struct Page;

impl MediaRanges for Page {
    const RANGES: &'static [&'static str] = &["text/html; charset"];
}

fn main() {
    let _ = Served::<_, Page>::new(InMemory::new(Bytes::new()));
}
