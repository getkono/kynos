//! The passing half of `routes_builder_method` and `routes_member_not_a_handler`.
//!
//! Every member names a handler, and the only method after one is
//! `intercept`, which scopes the interceptor to that operation alone.

use kynos::{middleware::limits::body_size::BodySize, prelude::*};

#[kynos::get("/users")]
async fn list() {}

#[kynos::post("/users")]
async fn create() {}

fn main() {
    let _ = Router::<()>::new().mount(kynos::routes![list, create.intercept(BodySize::new(1024))]);
}
