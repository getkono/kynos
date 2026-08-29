//! `catch_panics` does not launder a colliding interceptor.
//!
//! It changes the panic policy and nothing else about what the router carries,
//! so the interceptors mounted before it still cover every operation mounted
//! after it. Returning `Router<C, Catch>` -- which is `Router<C, Catch, ()>` --
//! would drop the type-level stack on the floor, and the second `RequestId`
//! would be checked against an empty list while both wrote `x-request-id` to
//! the same response.
//!
//! Two `RequestId::new()` rather than a hand-written pair: one type against
//! itself is the shortest collision there is, since `NAMES` is never disjoint
//! from itself.

use kynos::{middleware::request_id::RequestId, prelude::*};

fn main() {
    let _ = Router::<()>::new()
        .intercept(RequestId::new())
        .catch_panics()
        .intercept(RequestId::new());
}
