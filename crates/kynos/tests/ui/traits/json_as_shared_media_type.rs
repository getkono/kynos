//! Two `JsonAs` bodies under one type and subtype cannot be alternatives.
//!
//! Their markers differ only in a parameter, which a request need not repeat,
//! so a bare `application/vnd.acme.order+json` would reach both and dispatch
//! order would decide which won. The pair is refused in a `const` `OneOf`
//! evaluates, so the build fails rather than the description.

use kynos::{
    extract::body::{OneOf, json_as::JsonAs},
    http::media::MediaType,
    prelude::*,
};

struct V1;

impl MediaType for V1 {
    const MEDIA_TYPE: &'static str = "application/vnd.acme.order+json; version=1";
}

struct V2;

impl MediaType for V2 {
    const MEDIA_TYPE: &'static str = "application/vnd.acme.order+json; version=2";
}

#[kynos::post("/orders")]
async fn create(_order: OneOf<JsonAs<V1, u32>, JsonAs<V2, String>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![create]);
}
