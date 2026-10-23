//! The control for `traits/json_as_shared_media_type.rs`: the same handler and
//! the same mount, differing only in that the version is in the subtype.

use kynos::{
    extract::body::{OneOf, json_as::JsonAs},
    http::media::MediaType,
    prelude::*,
};

struct V1;

impl MediaType for V1 {
    const MEDIA_TYPE: &'static str = "application/vnd.acme.order.v1+json";
}

struct V2;

impl MediaType for V2 {
    const MEDIA_TYPE: &'static str = "application/vnd.acme.order.v2+json";
}

#[kynos::post("/orders")]
async fn create(_order: OneOf<JsonAs<V1, u32>, JsonAs<V2, String>>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![create]);
}
