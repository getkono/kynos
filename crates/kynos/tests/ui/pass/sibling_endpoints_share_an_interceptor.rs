//! The passing half of
//! `endpoint_interceptor_then_router_interceptor_collide`.
//!
//! Two endpoints mounted together, each with its own `RequestId`, and no
//! router-scoped interceptor. It differs in exactly the property under test --
//! whether the second stack covers the first's operations -- and it is the
//! shape `tests/limits.rs` relies on for a per-endpoint limit, so remembering
//! an endpoint's stack must leave it compiling.

use kynos::{middleware::request_id::RequestId, prelude::*};

#[kynos::get("/users")]
async fn list() {}

#[kynos::get("/orders")]
async fn orders() {}

fn main() {
    let _ = Router::<()>::new().mount((
        kynos::routes![list].0.intercept(RequestId::new()),
        kynos::routes![orders].0.intercept(RequestId::new()),
    ));
}
