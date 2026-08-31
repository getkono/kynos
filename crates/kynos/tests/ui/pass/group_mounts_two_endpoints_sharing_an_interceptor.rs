//! The passing half of `group_mount_then_group_intercept_collide`.
//!
//! One group, two mounts, each endpoint carrying its own `RequestId`, and no
//! group-scoped interceptor over them. It differs in exactly the property
//! under test, and it is what proves `mount` folds a stack into the group's
//! *sub*-stack rather than into the group's own: folding into the group's own
//! would compare the second endpoint against the first and refuse two
//! operations that no request reaches together.

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
