//! The passing half of `group_catch_panics_keeps_the_mounted_scopes`.
//!
//! The same shape -- mount, `catch_panics`, intercept -- differing in exactly
//! the property under test: 413 and 408 are disjoint and neither interceptor
//! adds a response header, so carrying `S` across the group's policy change
//! rejects nothing.

use kynos::{
    middleware::limits::{BodySize, Timeout},
    prelude::*,
};

#[kynos::get("/users")]
async fn list() {}

fn main() {
    let _ = Group::<()>::new("/a")
        .mount(kynos::routes![list].0.intercept(BodySize::new(1024)))
        .catch_panics()
        .intercept(Timeout::new(std::time::Duration::from_secs(30)));
}
