//! A forward names one context and the field path reaching this struct from
//! it, each once; a path that is more than fields, or that repeats the
//! `self` it already starts at, is refused, as is a context forwarded to
//! twice.

#[derive(Clone)]
struct Pool;

struct App {
    inner: Services,
}

#[derive(kynos::Provider)]
#[provider(for = App)]
struct NoPath {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(via = inner)]
struct NoContext {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = App, via = self.inner)]
struct StartsAtSelf {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = App, via = inner.services())]
struct CallsAMethod {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = App, via = inner)]
#[provider(for = App, via = inner)]
struct Services {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = App, via = inner, via = inner)]
struct PathTwice {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = App, through = inner)]
struct UnknownKey {
    pool: Pool,
}

fn main() {}
