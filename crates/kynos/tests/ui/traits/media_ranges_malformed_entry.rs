//! A `MediaRanges` entry that is no `media-range` does not compile.
//!
//! RFC 9110 section 12.5.1 has `*/*` and `type/*` and no `*/subtype`, and each
//! entry names a `content` key of the description.

use bytes::Bytes;
use kynos::{
    http::media::MediaRanges,
    response::range::{served::Served, source::InMemory},
};

struct Video;

impl MediaRanges for Video {
    const RANGES: &'static [&'static str] = &["*/mp4"];
}

fn main() {
    let _ = Served::<_, Video>::new(InMemory::new(Bytes::new()));
}
