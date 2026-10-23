//! The control for `traits/encoded_shared_media_type.rs`,
//! `traits/encoded_beside_json.rs` and `traits/encoded_beside_binary.rs`: the
//! same pairs, differing only in that each side names a subtype of its own.

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

struct SignedTally;

impl BodyCodec<i64> for SignedTally {
    const MEDIA_TYPE: &'static str = "application/vnd.example.signed-tally";

    fn decode(_: &bytes::Bytes) -> Result<i64, DecodeError> {
        Ok(0)
    }

    fn encode(_: &i64, _: &mut impl std::io::Write) -> std::io::Result<()> {
        Ok(())
    }
}

struct RawTally;

impl MediaType for RawTally {
    const MEDIA_TYPE: &'static str = "application/vnd.example.raw-tally";
}

#[kynos::post("/tallies")]
async fn record(_tally: OneOf<Encoded<Tally, u64>, Encoded<SignedTally, i64>>) {}

#[kynos::post("/tallies/json")]
async fn record_json(_tally: OneOf<Json<u64>, Encoded<Tally, u64>>) {}

#[kynos::post("/tallies/raw")]
async fn record_raw(_tally: OneOf<Encoded<Tally, u64>, Binary<RawTally>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![record, record_json, record_raw]);
}
