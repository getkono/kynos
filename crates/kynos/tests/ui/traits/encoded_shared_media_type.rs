//! Two `Encoded` bodies under one type and subtype cannot be alternatives.
//!
//! The codecs differ, but a request names a body only by its media type, so
//! `application/vnd.example.tally` would reach both and dispatch order would
//! decide which won. The pair is refused in a `const` `OneOf` evaluates, so the
//! build fails rather than the description; one codec beside itself is the
//! same pair.

use kynos::{
    extract::body::{
        OneOf,
        codec::{BodyCodec, DecodeError, Encoded},
    },
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

struct SignedTally;

impl BodyCodec<i64> for SignedTally {
    const MEDIA_TYPE: &'static str = "Application/Vnd.Example.Tally; signed=true";

    fn decode(_: &bytes::Bytes) -> Result<i64, DecodeError> {
        Ok(0)
    }

    fn encode(_: &i64, _: &mut impl std::io::Write) -> std::io::Result<()> {
        Ok(())
    }
}

#[kynos::post("/tallies")]
async fn record(_tally: OneOf<Encoded<Tally, u64>, Encoded<SignedTally, i64>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![record]);
}
