//! Assembling an application context at runtime.
//!
//! Most applications derive `Provider` on a plain struct instead. This is
//! for contexts built from configuration, where the set of dependencies is
//! not known until the process starts.

use std::future::Future;

/// How long a provided value lives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// Built once when the context is built, and shared by every request.
    ///
    /// The default, and the right choice for connection pools, HTTP clients and
    /// configuration.
    #[default]
    Singleton,

    /// Built at most once per request, lazily.
    ///
    /// For values that are per-request by nature — a database transaction, a
    /// request-scoped cache — and that most handlers do not need, so paying for
    /// them on every request would be waste.
    Request,
}

/// Builds an application context.
///
/// Most applications derive `Provider` on a plain struct instead of using this;
/// it exists for contexts assembled at runtime from configuration.
#[derive(Debug, Default)]
pub struct ContextBuilder<C> {
    _private: std::marker::PhantomData<C>,
}

impl<C> ContextBuilder<C> {
    /// Creates an empty builder.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }

    /// Provides `value` as a singleton.
    #[must_use]
    pub fn provide<T: Clone + Send + Sync + 'static>(self, value: T) -> Self {
        let _ = value;
        todo!()
    }

    /// Provides a value built lazily, once per request.
    #[must_use]
    pub fn provide_scoped<T, F, Fut>(self, factory: F) -> Self
    where
        T: Send + 'static,
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = T> + Send,
    {
        let _ = factory;
        todo!()
    }

    /// Finalizes the context.
    #[must_use]
    pub fn build(self) -> C {
        todo!()
    }
}
