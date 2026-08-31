//! The group's half of what `catch_panics_keeps_the_mounted_scopes` pins.
//!
//! `Group::catch_panics` already carries `S`, and this case exists so that it
//! keeps doing so. The router's half did not: one commit added `S`, updated the
//! group's return type and left the router's naming three parameters, and the
//! omission was invisible until a program that should have been refused
//! compiled. The two halves are edited together and only one of them was
//! covered, which is the drift this pins rather than a bug it reports.
//!
//! `group_mount_then_group_intercept_collide` is this program without the
//! `catch_panics`.

use kynos::{middleware::request_id::RequestId, prelude::*};

#[kynos::get("/users")]
async fn list() {}

fn main() {
    let _ = Group::<()>::new("/a")
        .mount(kynos::routes![list].0.intercept(RequestId::new()))
        .catch_panics()
        .intercept(RequestId::new());
}
