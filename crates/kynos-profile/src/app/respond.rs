//! The response group's handlers: `plaintext`'s octets as a ranged binary
//! representation, and `json-small`'s document negotiated or created.

use bytes::Bytes;
use kynos::{
    error::rejection::NegotiationRejection,
    extract::body::text::Text,
    http::{etag::ETag, media::OctetStream},
    prelude::*,
    response::{
        negotiate::{Accept, Negotiated},
        range::{
            served::{Conditions, Delivery, Served},
            source::InMemory,
        },
    },
};

use super::{App, Small, small};

/// The entity tag `/binary` is served under, as `If-None-Match` names it.
pub(crate) const BINARY_TAG: &str = "\"plaintext-v1\"";

/// `plaintext`'s octets, as a representation that answers ranges and
/// preconditions.
#[kynos::get("/binary")]
async fn binary(conditions: Conditions) -> Delivery<OctetStream> {
    Served::new(InMemory::new(Bytes::from_static(b"Hello, World!")))
        .etag(ETag::strong("plaintext-v1"))
        .deliver(&conditions)
        .await
        .unwrap_or_else(|never| match never {})
}

/// The representations `/json/negotiated` offers, JSON first.
type Representations = (Json<Small>, Text);

/// `json-small`'s document, chosen from two representations by `Accept`.
#[kynos::get("/json/negotiated")]
async fn negotiated(
    accept: Accept<Representations>,
) -> Result<Negotiated<Representations>, NegotiationRejection> {
    accept.respond_with(
        &small(),
        (
            |small: &Small| Json(small.clone()),
            |small: &Small| Text(small.name.clone()),
        ),
    )
}

/// `json-small`'s document, as a 201 naming where it lives.
#[kynos::get("/json/created")]
async fn created() -> Created<Json<Small>> {
    Created::at("/json/small", Json(small()))
}

/// Mounts this group's handlers.
pub(crate) fn mount(router: Router<App>) -> Router<App> {
    router.mount(kynos::routes![binary, negotiated, created])
}
