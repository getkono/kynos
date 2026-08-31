//! The passing half of `group_interceptor_then_router_interceptor_collide`.
//!
//! The same two calls in the same order, differing in exactly the property
//! under test: `BodySize` answers 413 and adds no response header, so it
//! collides with the group's `RequestId` in neither of the two ways
//! `CompatibleWith` compares.
//!
//! Remembering a group's stack must reject the colliding pair and nothing
//! else; without this control it could reject both and still pass.

use kynos::{
    middleware::{limits::BodySize, request_id::RequestId},
    prelude::*,
};

fn main() {
    let _ = Router::<()>::new()
        .group(Group::<()>::new("/a").intercept(RequestId::new()))
        .intercept(BodySize::new(1024));
}
