//! The control for `traits/binary_malformed_media_type.rs`: a vendor marker
//! naming a `type/subtype` compiles. Only the media type differs.

use kynos::{extract::body::binary::Binary, http::media::MediaType, prelude::*};

struct Tally;

impl MediaType for Tally {
    const MEDIA_TYPE: &'static str = "application/vnd.example.tally";
}

#[kynos::get("/tallies/current")]
async fn current() -> Binary<Tally> {
    Binary::new("7")
}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![current]);
}
