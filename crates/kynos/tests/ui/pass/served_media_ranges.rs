//! A `MediaRanges` set a downstream crate declares delivers once it is given a
//! type, and mounts as a route: the sibling of the three `ui/traits/` cases
//! refusing an empty set, a malformed entry, and a set given no type.
//!
//! Downstream, because the blanket implementation over `MediaType` is what such
//! an `impl` could collide with. A marker beside it still delivers as it did.

use bytes::Bytes;
use kynos::{
    http::media::{MediaRanges, OctetStream},
    prelude::*,
    response::range::{
        served::{Conditions, Delivery, Served},
        source::InMemory,
    },
};

struct Video;

impl MediaRanges for Video {
    const RANGES: &'static [&'static str] = &["video/*"];
}

#[kynos::get("/clips/current")]
async fn clip(conditions: Conditions) -> Delivery<Video> {
    Served::<_, Video>::new(InMemory::new(Bytes::new()))
        .content_type("video/mp4")
        .deliver(&conditions)
        .await
        .expect("an in-memory source cannot fail")
}

#[kynos::get("/files/current")]
async fn file(conditions: Conditions) -> Delivery<OctetStream> {
    Served::<_, OctetStream>::new(InMemory::new(Bytes::new()))
        .deliver(&conditions)
        .await
        .expect("an in-memory source cannot fail")
}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![clip, file]);
}
