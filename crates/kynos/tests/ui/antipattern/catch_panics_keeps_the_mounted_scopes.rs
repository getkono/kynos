//! `catch_panics` does not launder a group's interceptor either.
//!
//! The sibling of `catch_panics_keeps_the_interceptor_stack`, one parameter
//! over. That case pins `I`, the router's own interceptors; this one pins `S`,
//! what the scopes mounted here brought with them. Returning
//! `Router<C, Catch, I>` -- which is `Router<C, Catch, I, ()>` -- drops `S`, so
//! the group's `RequestId` is compared against an empty list while both write
//! `x-request-id` to every response under `/a`.
//!
//! `group_interceptor_then_router_interceptor_collide` is this program without
//! the `catch_panics` and it is refused, so the policy change is the whole
//! difference between a caught collision and a silent one.

use kynos::{middleware::request_id::RequestId, prelude::*};

fn main() {
    let _ = Router::<()>::new()
        .group(Group::<()>::new("/a").intercept(RequestId::new()))
        .catch_panics()
        .intercept(RequestId::new());
}
