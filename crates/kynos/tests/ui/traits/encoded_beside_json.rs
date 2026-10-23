//! An `Encoded` whose codec names `application/json` cannot sit beside a
//! `Json`.
//!
//! A request carrying `application/json` would reach both, so the pair is
//! refused in a `const` `OneOf` evaluates, as `Binary<media::Json>` is.

use kynos::{
    extract::body::{
        OneOf,
        codec::{BodyCodec, DecodeError, Encoded},
    },
    prelude::*,
};

struct OtherJson;

impl BodyCodec<u64> for OtherJson {
    const MEDIA_TYPE: &'static str = "application/json";

    fn decode(_: &bytes::Bytes) -> Result<u64, DecodeError> {
        Ok(0)
    }

    fn encode(_: &u64, _: &mut impl std::io::Write) -> std::io::Result<()> {
        Ok(())
    }
}

#[kynos::post("/tallies")]
async fn record(_tally: OneOf<Json<u64>, Encoded<OtherJson, u64>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![record]);
}
