//! The control for `macros/provider_via_conflict`: the same context, its own
//! `Pool` skipped, so the forwarded one is the only source.

use std::sync::Arc;

#[derive(Clone)]
struct Pool;

#[derive(Clone, kynos::Provider)]
struct App {
    #[provide(skip)]
    #[allow(dead_code)]
    pool: Pool,
    #[provide(skip)]
    inner: Arc<Services>,
}

#[derive(kynos::Provider)]
#[provider(for = App, via = inner)]
struct Services {
    pool: Pool,
}

fn main() {}
