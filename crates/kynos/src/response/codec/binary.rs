//! Writing raw bytes with a declared media type as a response.

use crate::{
    extract::body::binary::Binary,
    http::{
        HeaderValue, Response,
        body::Body,
        media::{MediaType, WellFormed},
    },
    response::{
        IntoResponse, Responses,
        codec::{ok, unconstrained, with_content_type},
    },
    schema::registry::Registry,
};

impl<M: MediaType> IntoResponse for Binary<M> {
    fn into_response(self) -> Response {
        let () = <M as WellFormed>::WELL_FORMED;

        with_content_type(
            Body::from_bytes(self.bytes),
            HeaderValue::from_static(M::MEDIA_TYPE),
        )
    }
}

impl<M: MediaType> Responses for Binary<M> {
    fn responses(_registry: &mut Registry) -> kynos_openapi::Responses {
        let () = <M as WellFormed>::WELL_FORMED;
        ok(M::MEDIA_TYPE, unconstrained())
    }
}
