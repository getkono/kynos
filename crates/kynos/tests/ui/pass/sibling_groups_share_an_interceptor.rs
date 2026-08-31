//! Two groups mounted side by side may hold the same interceptor.
//!
//! The false positive the sub-stack check must not introduce. Remembering what
//! a router has mounted is only sound while the remembered stacks are compared
//! against a *newcomer* and never against each other: no request reaches two
//! groups, so two `RequestId` covering disjoint operations is not a collision
//! and never was.
//!
//! `CompatibleWith for Cons<H, T>` compares the newcomer against `H` and
//! recurses on `T` with the same newcomer, so members of one list are never
//! compared to one another. This case is what holds that reading.

use kynos::{middleware::request_id::RequestId, prelude::*};

fn main() {
    let _ = Router::<()>::new()
        .group(Group::<()>::new("/a").intercept(RequestId::new()))
        .group(Group::<()>::new("/b").intercept(RequestId::new()));
}
