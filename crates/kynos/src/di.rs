//! Dependency injection.
//!
//! Application state has no effect on the wire, so this is the one part of
//! Kynos where the describability constraint does not apply — and therefore the
//! one part where Kynos can be more capable than its peers at no cost to the
//! thesis.
//!
//! The design goal is that a missing dependency is a **compile error**. Axum,
//! actix-web and poem all resolve erased state at runtime and panic when it is
//! absent; salvo's `Depot` is a stringly-typed map. Here, a handler asking for
//! `Inject<Db>` where the context provides no `Db` fails to typecheck.
//!
//! ```compile_fail
//! struct Db;
//! struct App;
//!
//! // `App` implements `Provides<Db>` for nothing, so a handler taking
//! // `Inject<Db>` against this context does not typecheck.
//! fn resolvable<C: kynos::di::Provides<Db>>() {}
//! resolvable::<App>();
//! ```
//!
//! # What is *not* a dependency
//!
//! A value read from the request is not a dependency, however convenient it
//! would be to treat it as one. `CurrentUser` derived from an `Authorization`
//! header is a [`SecurityScheme`](crate::security::SecurityScheme), and reaches
//! a handler through [`Auth`](crate::security::Auth), so that requiring it also
//! documents it. Injecting it would make the requirement invisible.

use std::future::Future;

/// A context that can supply a `T`.
///
/// Derived by `#[derive(Provider)]`, which emits one implementation per field.
pub trait Provides<T> {
    /// Supplies the value for one request.
    fn provide(&self) -> impl Future<Output = T> + Send;
}

/// A value derived from the application context rather than the request.
///
/// The counterpart to [`FromRequestParts`](crate::extract::FromRequestParts):
/// implementing this says "I contribute nothing to the description", and there
/// is deliberately no way for one type to do both.
pub trait FromContext<C>: Sized + Send {
    /// Builds the value from the context.
    fn from_context(context: &C) -> impl Future<Output = Self> + Send;
}

/// A dependency resolved from the application context.
///
/// ```no_run
/// # use kynos::di::Inject;
/// # struct Db;
/// async fn list_users(Inject(db): Inject<Db>) {
///     todo!()
/// }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Inject<T>(pub T);

impl<T> Inject<T> {
    /// Unwraps the injected value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<C, T> FromContext<C> for Inject<T>
where
    C: Provides<T> + Sync,
    T: Send,
{
    async fn from_context(context: &C) -> Self {
        let _ = context;
        todo!()
    }
}

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
