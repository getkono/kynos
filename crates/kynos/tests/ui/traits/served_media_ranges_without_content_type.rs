//! A delivery named by a `MediaRanges` set does not compile until it is given a
//! type.
//!
//! A set names no type to send, so `deliver` exists only after `content_type`:
//! the other way to answer this would be a run-time refusal of every request.

use bytes::Bytes;
use kynos::{
    http::media::MediaRanges,
    response::range::{
        served::{Conditions, Delivery, Served},
        source::InMemory,
    },
};

struct Video;

impl MediaRanges for Video {
    const RANGES: &'static [&'static str] = &["video/*"];
}

async fn clip(conditions: Conditions) -> Delivery<Video> {
    Served::<_, Video>::new(InMemory::new(Bytes::new()))
        .deliver(&conditions)
        .await
        .expect("an in-memory source cannot fail")
}

fn main() {
    let _ = clip;
}
