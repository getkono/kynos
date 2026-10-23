//! A `JsonAs` whose marker does not name JSON does not compile.
//!
//! The check is a `const` on `JsonAs`, forced wherever the body is extracted,
//! written, described or offered, so the build fails rather than a body
//! refusing every request with 415. A `+xml` suffix is the readable mistake: a
//! structured document, but not one JSON decodes.

use kynos::{extract::body::json_as::JsonAs, http::media::MediaType, prelude::*};

struct Order;

impl MediaType for Order {
    const MEDIA_TYPE: &'static str = "application/vnd.acme.order+xml";
}

#[kynos::post("/orders")]
async fn create(_order: JsonAs<Order, u32>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![create]);
}
