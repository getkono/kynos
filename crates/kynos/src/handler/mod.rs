//! The bridge from an `async fn` to an operation.

use std::future::Future;

use crate::{
    http::{Request, Response},
    router::operation::OperationCx,
};

// Implementations only, so there is no item here for a canonical path to point
// at. Keeping it private is also what stops the arity macro leaking.
mod impls;

/// Marks a handler whose last argument consumes the request body.
///
/// Occupies the first slot of a handler's argument tuple. Never written by
/// hand: it exists so that the body-consuming and head-only implementations do
/// not overlap, since a function of `n` arguments would otherwise match both.
#[derive(Debug)]
pub enum ViaRequest {}

/// Marks a handler that reads only the request head.
#[derive(Debug)]
pub enum ViaParts {}

/// Marks a handler whose first argument is a [`Guard`](crate::security::Guard).
///
/// Leads the argument tuple, before [`ViaRequest`] or [`ViaParts`], so that the
/// guarded and unguarded implementations do not overlap either.
#[derive(Debug)]
pub enum Guarded {}

/// An `async fn` usable as an operation handler.
///
/// Implemented for functions of up to sixteen extractors where every extractor
/// but the last implements
/// [`FromRequestParts`](crate::extract::FromRequestParts) and
/// [`Describe`](crate::extract::describe::Describe), the last implements either
/// of those or [`FromRequest`](crate::extract::FromRequest), and the return
/// type implements [`IntoResponse`](crate::response::IntoResponse) and
/// [`Responses`](crate::response::Responses) — optionally after one
/// [`Guard`](crate::security::Guard), which takes the first slot the way a body
/// takes the last.
///
/// The bounds are the whole enforcement mechanism. An argument that cannot
/// implement `Describe` — a raw request, a whole header map, an untyped body —
/// has no way into a handler signature, and a return type that cannot
/// implement `Responses` has no way out. There is no implementation with two
/// guards, so an operation cannot demand two requirements its description
/// would list as alternatives.
///
/// `A` is `(Marker, T1, .., Tn)`: a [`ViaRequest`] or [`ViaParts`] marker
/// followed by the argument types, or `()` for a handler that takes none. A
/// guarded handler's is `(Guarded, Marker, G, T1, .., Tn)`, or `(Guarded, G)`
/// when the guard is its only argument. It carries no information a caller
/// supplies — it is inferred at every call site — and exists only so the
/// implementations are disjoint.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a Kynos handler",
    label = "not a handler",
    note = "every argument must implement `Describe`, and `FromRequestParts` — or `FromRequest`, \
            for the last one",
    note = "a guard — `Auth`, `MaybeAuth` or `Scoped` — is the first argument, and there is at \
            most one: combine schemes with `AnyOf` or `AllOf` instead",
    note = "the return type must implement `IntoResponse` and `Responses`, and a codec such as \
            `Json<T>` implements `Responses` only when `T` implements `Schema`: \
            `#[derive(kynos::Schema)]` on `T` is the usual fix",
    note = "the handler's future must be `Send`: nothing that is not — an `Rc`, a `RefCell` \
            borrow, a lock guard — may be held across an `.await`"
)]
pub trait Handler<C, A>: Clone + Send + Sync + 'static {
    /// Runs the handler: extracts every argument, then invokes it.
    ///
    /// The context is borrowed for the life of the request. One exists per
    /// process, and extraction never clones it.
    fn call(self, request: Request, context: &C) -> impl Future<Output = Response> + Send;

    /// Describes the handler's inputs and outputs into the operation.
    ///
    /// Contributes, in order: each argument's
    /// [`Describe`](crate::extract::describe::Describe), guard first; each
    /// argument's rejection responses; the return type's
    /// [`Responses`](crate::response::Responses).
    ///
    /// The rejection half happens here rather than in each `Describe`
    /// implementation because `Rejection` is chosen per context type, which
    /// `Describe` cannot name — this is the only place the context and the
    /// argument type are both in scope. Putting it here is what makes
    /// *emitted ⊇ observable* mechanical instead of a convention every
    /// extractor author has to remember.
    fn describe(operation: &mut OperationCx<'_>);
}
