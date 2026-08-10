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
//! a handler through [`Auth`](crate::security::auth::Auth), so that requiring it also
//! documents it. Injecting it would make the requirement invisible.
//!
//! # How this module is laid out
//!
//! The two traits live here; [`inject`] holds the wrapper a handler receives a
//! resolved value in, and [`context`] the runtime builder and its scopes.

pub mod context;
pub mod inject;

use std::future::Future;

/// A context that can supply a `T`.
///
/// Derived by `#[derive(Provider)]`, which emits one implementation per field.
#[diagnostic::on_unimplemented(
    message = "the context `{Self}` provides no `{T}`",
    label = "cannot supply `{T}`",
    note = "add a `{T}` field to the context type and `#[derive(kynos::Provider)]`, or write \
            `impl Provides<{T}> for {Self}` by hand",
    note = "a dependency a handler asks for and the context does not have is a compile error \
            here rather than a panic in production, which is the whole point of `Inject`"
)]
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
