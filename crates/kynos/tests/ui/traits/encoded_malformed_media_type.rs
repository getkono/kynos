//! A `BodyCodec` whose `MEDIA_TYPE` is not a `type/subtype` does not compile.
//!
//! The check is a `const` on `Encoded`, forced wherever the body is extracted,
//! written, described or offered, so the build fails rather than a body no
//! request can name. A bare subtype is the readable mistake.

use kynos::{
    extract::body::codec::{BodyCodec, DecodeError, Encoded},
    prelude::*,
};

struct Tally;

impl BodyCodec<u64> for Tally {
    const MEDIA_TYPE: &'static str = "vnd.example.tally";

    fn decode(_: &bytes::Bytes) -> Result<u64, DecodeError> {
        Ok(0)
    }

    fn encode(_: &u64, _: &mut impl std::io::Write) -> std::io::Result<()> {
        Ok(())
    }
}

#[kynos::post("/tallies")]
async fn record(_tally: Encoded<Tally, u64>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![record]);
}
