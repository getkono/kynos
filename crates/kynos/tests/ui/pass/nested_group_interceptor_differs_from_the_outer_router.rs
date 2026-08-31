//! The passing half of
//! `nested_group_interceptor_collides_with_the_outer_router`.
//!
//! The same nesting, differing in exactly the property under test: the outer
//! `BodySize` answers 413 and adds nothing, so it collides with the nested
//! group's `RequestId` neither on a header nor on a status.

use kynos::{
    middleware::{limits::BodySize, request_id::RequestId},
    prelude::*,
};

fn main() {
    let _ = Router::<()>::new().intercept(BodySize::new(1024)).nest(
        "/x",
        Router::<()>::new().group(Group::<()>::new("/y").intercept(RequestId::new())),
    );
}
