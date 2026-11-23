//! A type both the context and the struct forwarded to it provide is
//! reported at both sources, since a handler asking for it could not say
//! which it meant.

use std::sync::Arc;

#[derive(Clone)]
struct Pool;

#[derive(Clone, kynos::Provider)]
struct App {
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
