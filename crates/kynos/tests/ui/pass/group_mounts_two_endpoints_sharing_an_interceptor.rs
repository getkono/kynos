//! One group, two mounts, each endpoint carrying its own `RequestId`, and no
//! group-scoped interceptor over them.
//!
//! The false positive `mount` must not introduce: it checks what the endpoints
//! hold against the group's own interceptors and nothing else, so the second
//! endpoint is never compared against the first, and two operations no request
//! reaches together compile. `tests/collisions.rs` holds the same program to
//! building, where the router compares what is no longer in the group's type.

use kynos::{middleware::request_id::RequestId, prelude::*};

#[kynos::get("/users")]
async fn list() {}

#[kynos::get("/orders")]
async fn orders() {}

fn main() {
    let _ = Group::<()>::new("/a")
        .mount(kynos::routes![list].0.intercept(RequestId::new()))
        .mount(kynos::routes![orders].0.intercept(RequestId::new()));
}
