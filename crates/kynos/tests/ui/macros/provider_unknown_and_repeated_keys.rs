//! The `#[provide(...)]` grammar is closed and reads each key once: a
//! misspelled `skip` once provided the field anyway, and a second `skip` is a
//! key written twice.

use std::time::Instant;

#[derive(Clone)]
struct Pool;

#[derive(kynos::Provider)]
struct App {
    pool: Pool,
    #[provide(skp)]
    started_at: Instant,
}

#[derive(kynos::Provider)]
struct Worker {
    pool: Pool,
    #[provide(skip)]
    #[provide(skip)]
    started_at: Instant,
}

fn main() {}
