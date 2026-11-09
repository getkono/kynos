//! The response group's handlers: `plaintext`'s octets as a ranged binary
//! representation, `json-small`'s document negotiated or created, and
//! `json-small`'s and `json-large`'s documents serialized by `serde_json`
//! alone.

use bytes::Bytes;
use kynos::{
    error::rejection::NegotiationRejection,
    extract::body::{binary::Binary, text::Text},
    http::{
        etag::ETag,
        media::{Json as JsonMedia, OctetStream},
    },
    prelude::*,
    response::{
        negotiate::{Accept, Negotiated},
        range::{
            served::{Conditions, Delivery, Served},
            source::InMemory,
        },
    },
};

use super::{App, Small, large, small};

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
    Created::at(Location::from_static("/json/small"), Json(small()))
}

/// `value` as `serde_json::to_vec` writes it, under `application/json`: what
/// an application spends serializing with no framework in between, against
/// which [`Json`]'s own writer is read.
fn bare(value: &impl serde::Serialize) -> Binary<JsonMedia> {
    Binary::new(serde_json::to_vec(value).expect("a struct of plain fields serializes"))
}

/// `json-small`'s document, serialized by `serde_json` alone.
#[kynos::get("/json/small/bare")]
async fn json_small_bare() -> Binary<JsonMedia> {
    bare(&small())
}

/// `json-large`'s document, serialized by `serde_json` alone.
#[kynos::get("/json/large/bare")]
async fn json_large_bare() -> Binary<JsonMedia> {
    bare(&large())
}

/// Mounts this group's handlers.
pub(crate) fn mount(router: Router<App>) -> Router<App> {
    router.mount(kynos::routes![
        binary,
        negotiated,
        created,
        json_small_bare,
        json_large_bare
    ])
}
