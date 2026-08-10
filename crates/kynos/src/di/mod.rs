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
//! # The context is a type, not a map
//!
//! There is no container to register into and no builder to assemble. The
//! application's own struct *is* the context: it is handed to
//! [`Router::build`](crate::Router::build) once, and a handler's requirements
//! are bounds on it. Resolution is therefore a trait selection the compiler
//! performs, and there is nowhere for a lookup to fail at run time.
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
//! A value read from the request is not a dependency, however convenient it
//! would be to treat it as one. `CurrentUser` derived from an `Authorization`
//! header is a [`SecurityScheme`](crate::security::SecurityScheme), and reaches
//! a handler through [`Auth`](crate::security::auth::Auth), so that requiring it
//! also documents it. Injecting it would make the requirement invisible.
//!
//! # Why resolution is synchronous and cannot fail
//!
//! An injected value contributes nothing to the description — that is what
//! makes injection free of the describability constraint. It follows that
//! injection must not be able to *produce* anything a consumer could observe,
//! and a failure is observable: it becomes a response.
//!
//! So acquisition that can fail or block is not injection. Inject the *handle*
//! — a pool, a client, a channel — and perform the acquisition in the handler
//! body, where its failure lands in the return type and therefore in the
//! description. This is not a limitation working around a missing feature; a
//! fallible provider would produce responses no operation declares, which is
//! the one thing this framework exists to prevent.
//!
//! # Scope
//!
//! Every provider is a singleton for the life of the process: one context
//! exists, and [`Provides::provide`] hands out a value from it per request.
//!
//! Per-request memoization — one database transaction shared by two injected
//! repositories — is deliberately absent rather than pending. The 90% case
//! needs nothing from Kynos: inject the pool and open the transaction where it
//! is used. If a first-class version is ever wanted it is purely additive, and
//! costs no signature change today: a `ProvidesScoped<T>` capability plus a new
//! extractor, with the memo living in the request's own extensions, which
//! [`FromRequestParts`](crate::extract::FromRequestParts) already hands every
//! extractor mutably. A miss there is a cold cache rather than a missing
//! dependency, so it still cannot panic and the compile-time guarantee still
//! comes from the bound on the context.
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
///
/// This is the capability-trait shape: a handler names what it needs, and the
/// context proves it can supply it. Nothing is registered, looked up or erased.
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
