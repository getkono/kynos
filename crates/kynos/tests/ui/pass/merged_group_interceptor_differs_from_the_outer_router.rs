//! The passing half of
//! `merged_group_interceptor_collides_with_the_outer_router`.
//!
//! The same merge, differing in exactly the property under test: 413 against
//! a group that adds `x-request-id` and answers with nothing.

use kynos::{
    middleware::{limits::BodySize, request_id::RequestId},
    prelude::*,
};

fn main() {
    let _ = Router::<()>::new()
        .intercept(BodySize::new(1024))
        .merge(Router::<()>::new().group(Group::<()>::new("/y").intercept(RequestId::new())));
}
