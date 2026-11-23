//! The control for `macros/provider_via_grammar`: the same forwards, each
//! naming one context and a field path once. Each forwards to a context of
//! its own, since two structs providing one type to a context conflict.

#[derive(Clone)]
struct Pool;

macro_rules! context {
    ($($context:ident holds $inner:ident),+ $(,)?) => {
        $( struct $context { inner: $inner } )+
    };
}

context! {
    ForNoPath holds NoPath,
    ForNoContext holds NoContext,
    ForStartsAtSelf holds StartsAtSelf,
    ForCallsAMethod holds CallsAMethod,
    ForServices holds Services,
    ForPathTwice holds PathTwice,
    ForUnknownKey holds UnknownKey,
}

/// A second context, which a repeated attribute forwards to.
struct Other {
    inner: Services,
}

#[derive(kynos::Provider)]
#[provider(for = ForNoPath, via = inner)]
struct NoPath {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = ForNoContext, via = inner)]
struct NoContext {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = ForStartsAtSelf, via = inner)]
struct StartsAtSelf {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = ForCallsAMethod, via = inner)]
struct CallsAMethod {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = ForServices, via = inner)]
#[provider(for = Other, via = inner)]
struct Services {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = ForPathTwice, via = inner)]
struct PathTwice {
    pool: Pool,
}

#[derive(kynos::Provider)]
#[provider(for = ForUnknownKey, via = inner)]
struct UnknownKey {
    pool: Pool,
}

fn main() {}
