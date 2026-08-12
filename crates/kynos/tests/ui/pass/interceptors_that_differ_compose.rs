//! The passing half of `interceptors_collide_on_a_status` and
//! `interceptors_collide_on_a_header`.
//!
//! Four interceptors on one router: three answering with different statuses,
//! and one adding a header none of them touches. Nothing collides, so the
//! stack composes.
//!
//! Guarded by `black_box(false)` because the router is still `todo!()` — the
//! property under test is that this *compiles*, per the compile-only pattern in
//! `docs/testing.md`.

use std::hint::black_box;

use kynos::{
    middleware::{
        limits::{BodySize, Concurrency, Timeout},
        request_id::RequestId,
    },
    prelude::*,
};

fn main() {
    if black_box(false) {
        let _ = Router::<()>::new()
            // 413, 504 and 503 -- pairwise disjoint.
            .intercept(BodySize::new(1024))
            .intercept(Timeout::new(std::time::Duration::from_secs(30)))
            .intercept(Concurrency::new(256))
            // `x-request-id`, which none of the three above adds.
            .intercept(RequestId::new());
    }
}
