//! The control for `traits/json_as_non_json_marker.rs`: the same handler and
//! the same mount, differing only in that the marker's suffix is `+json`.

use kynos::{extract::body::json_as::JsonAs, http::media::MediaType, prelude::*};

struct Order;

impl MediaType for Order {
    const MEDIA_TYPE: &'static str = "application/vnd.acme.order+json";
}

#[kynos::post("/orders")]
async fn create(_order: JsonAs<Order, u32>) {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![create]);
}
