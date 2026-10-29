//! A `MediaType` marker whose `MEDIA_TYPE` is not a `type/subtype` does not
//! compile.
//!
//! The check is a `const` every marker carries, forced wherever a body naming
//! it is extracted, written, described or offered, so the build fails rather
//! than the first response panicking or going out without a `Content-Type`. A
//! bare subtype is the readable mistake.

use kynos::{extract::body::binary::Binary, http::media::MediaType, prelude::*};

struct Tally;

impl MediaType for Tally {
    const MEDIA_TYPE: &'static str = "vnd.example.tally";
}

#[kynos::get("/tallies/current")]
async fn current() -> Binary<Tally> {
    Binary::new("7")
}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![current]);
}
