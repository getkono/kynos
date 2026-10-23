//! An `Encoded` whose first parameter is not a `BodyCodec` for its value does
//! not compile, and the diagnostic says what that parameter is for.
//!
//! The readable mistake is naming a media type marker, which states a media
//! type but converts nothing.

use kynos::{
    extract::{FromRequest, body::codec::Encoded},
    http::media::Png,
};

fn from_request<C, T: FromRequest<C>>() {}

fn main() {
    from_request::<(), Encoded<Png, u64>>();
}
