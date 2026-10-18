//! The control for `macros/provider_unknown_and_repeated_keys`: the same
//! contexts, differing only in that `skip` is spelled the way the grammar
//! defines it and written once.

use std::time::Instant;

#[derive(Clone)]
struct Pool;

#[derive(kynos::Provider)]
struct App {
    pool: Pool,
    #[provide(skip)]
    started_at: Instant,
}

#[derive(kynos::Provider)]
struct Worker {
    pool: Pool,
    #[provide(skip)]
    started_at: Instant,
}

fn main() {}
