//! A router interceptor is checked against the groups already mounted.
//!
//! `group` used to return `Self`, so the group's stack was checked against the
//! router's and then dropped from the type. A later `intercept` was therefore
//! compared against an empty list -- while at run time the router's chain and
//! the group's both cover every operation in the group, and both write
//! `x-request-id`.
//!
//! The reverse order was always refused, which is what made this an ordering
//! accident rather than a rule.

use kynos::{middleware::request_id::RequestId, prelude::*};

fn main() {
    let _ = Router::<()>::new()
        .group(Group::<()>::new("/a").intercept(RequestId::new()))
        .intercept(RequestId::new());
}
