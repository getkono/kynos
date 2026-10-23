//! The control for `traits/body_codec.rs` and
//! `traits/encoded_malformed_media_type.rs`: a codec for the value, under a
//! well-formed media type, extracts, responds and is offerable.

use kynos::{
    extract::{
        FromRequest,
        body::codec::{BodyCodec, DecodeError, Encoded},
        describe::RequestContent,
    },
    response::{IntoResponse, Responses, negotiate::representation::Representation},
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

fn from_request<C, T: FromRequest<C> + RequestContent>() {}
fn into_response<T: IntoResponse + Responses + Representation>() {}

fn main() {
    from_request::<(), Encoded<Tally, u64>>();
    into_response::<Encoded<Tally, u64>>();
    let _ = Encoded::<Tally, u64>::media_types();
}
