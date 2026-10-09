//! Dependency injection.
//!
//! A missing dependency is a **compile error**: a handler asking for
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
//! # The context is a type, not a map
//!
//! The application's own struct *is* the context: it is handed to
//! [`Router::build`](crate::Router::build) once, and a handler's requirements
//! are bounds on it, resolved by the compiler.
//!
//! ```no_run
//! # use kynos::di::Provides;
//! #[derive(Clone)]
//! struct Pool;
//!
//! struct App {
//!     pool: Pool,
//! }
//!
//! // What `#[derive(kynos::Provider)]` emits, one implementation per field.
//! impl Provides<Pool> for App {
//!     fn provide(&self) -> Pool {
//!         self.pool.clone()
//!     }
//! }
//! ```
//!
//! # What is *not* a dependency
//!
//! A value read from the request is not a dependency. `CurrentUser` derived
//! from an `Authorization` header is a
//! [`SecurityScheme`](crate::security::SecurityScheme), and reaches a handler
//! through [`Auth`](crate::security::auth::Auth), so that requiring it also
//! documents it.
//!
//! # Resolution is synchronous and cannot fail
//!
//! A failure would become a response no operation declares. Inject the
//! *handle* — a pool, a client, a channel — and perform fallible or blocking
//! acquisition in the handler body, where its failure lands in the return type
//! and therefore in the description.
//!
//! # Scope
//!
//! Every provider is a singleton for the life of the process: one context
//! exists, and [`Provides::provide`] hands out a value from it per request.
//! There is no per-request memoization; inject the pool and open the
//! transaction where it is used.
//!
//! # A provider hands out a value rather than lending one
//!
//! [`Provides::provide`] returns `T`, not `&T`, because
//! [`FromRequestParts::from_request_parts`](crate::extract::FromRequestParts::from_request_parts)
//! returns `Self` with no lifetime tying it to the context. The cost is one
//! clone of a handle per injected argument per request — one atomic increment
//! for an `Arc`. See [`docs/state.md`] for the reasoning.
//!
//! [`docs/state.md`]: https://github.com/getkono/kynos/blob/master/docs/state.md
//!
//! # How this module is laid out
//!
//! The trait lives here; [`inject`] holds the wrapper a handler receives a
//! resolved value in.

pub mod inject;

/// A context that can supply a `T`.
///
/// Normally derived by `#[derive(Provider)]`, which emits one implementation
/// per field. Implementations are expected to be cheap — typically a clone of a
/// handle — because one runs per injected argument per request.
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
    fn provide(&self) -> T;
}

/// Every context provides itself.
///
/// An application with one dependency can use the dependency as its own
/// context — `Router::<Arc<Pool>>::new()` satisfies `Inject<Arc<Pool>>` with no
/// derive and no wrapper struct.
impl<T: Clone> Provides<T> for T {
    fn provide(&self) -> T {
        self.clone()
    }
}

#[cfg(test)]
mod tests;
