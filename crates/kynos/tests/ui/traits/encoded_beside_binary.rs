//! An `Encoded` cannot sit beside a `Binary` of its own type and subtype.
//!
//! A typed codec decodes what a `Binary` would hand over undecoded, and a
//! request names the two the same way, so the pair is refused in a `const`
//! `OneOf` evaluates rather than left to dispatch order.

use kynos::{
    extract::body::{
        OneOf,
        binary::Binary,
        codec::{BodyCodec, DecodeError, Encoded},
    },
    http::media::MediaType,
    prelude::*,
};

struct Tally;

impl BodyCodec<u64> for Tally {
    const MEDIA_TYPE: &'static str = "application/vnd.example.tally";

    fn decode(_: &bytes::Bytes) -> Result<u64, DecodeError> {
        Ok(0)
    }

    fn encode(_: &u64, _: &mut impl std::io::Write) -> std::io::Result<()> {
        Ok(())
    }
}

struct RawTally;

impl MediaType for RawTally {
    const MEDIA_TYPE: &'static str = "application/vnd.example.tally";
}

#[kynos::post("/tallies")]
async fn record(_tally: OneOf<Encoded<Tally, u64>, Binary<RawTally>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![record]);
}
