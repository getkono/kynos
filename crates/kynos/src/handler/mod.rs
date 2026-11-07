//! The bridge from an `async fn` to an operation.

use std::future::Future;

use crate::{
    http::{Parts, Response, body::Body},
    router::operation::OperationCx,
};

// Private: implementations only, and it keeps the arity macro from leaking.
mod impls;

/// Marks a handler whose last argument consumes the request body.
///
/// Never written by hand: it keeps the body-consuming and head-only
/// implementations disjoint.
#[derive(Debug)]
pub enum ViaRequest {}

/// Marks a handler that reads only the request head.
#[derive(Debug)]
pub enum ViaParts {}

/// Marks a handler whose first argument is a [`Guard`](crate::security::Guard).
///
/// Leads the argument tuple, before [`ViaRequest`] or [`ViaParts`], keeping the
/// guarded and unguarded implementations disjoint.
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
/// An argument that cannot implement `Describe` — a raw request, a whole
/// header map, an untyped body — has no way into a handler signature, and a
/// return type that cannot implement `Responses` has no way out. A second
/// guard is a compile error.
///
/// `A` is `(Marker, T1, .., Tn)`: a [`ViaRequest`] or [`ViaParts`] marker
/// followed by the argument types, or `()` for a handler that takes none. A
/// guarded handler's is `(Guarded, Marker, G, T1, .., Tn)`, or `(Guarded, G)`
/// when the guard is its only argument. It is always inferred.
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
    /// Runs the handler: extracts every argument from the head it is lent and
    /// the body it is handed, then invokes it.
    ///
    /// The head is lent rather than handed over inside a request so that the
    /// future owning it, the endpoint's, holds the only copy: one passed by
    /// value would keep its slot in every state of this future. The context is
    /// borrowed for the life of the request, never cloned.
    fn call(
        self,
        parts: &mut Parts,
        body: Body,
        context: &C,
    ) -> impl Future<Output = Response> + Send;

    /// Describes the handler's inputs and outputs into the operation.
    ///
    /// Contributes, argument by argument and guard first, each argument's
    /// [`Describe`](crate::extract::describe::Describe) followed by its
    /// rejection's responses; then the return type's
    /// [`Responses`](crate::response::Responses).
    ///
    /// Rejections are described here, not in `Describe`, because `Rejection`
    /// depends on the context type, which `Describe` cannot name.
    fn describe(operation: &mut OperationCx<'_>);

    /// Whether the handler takes a guard, which
    /// [`__guard_ahead`](Handler::__guard_ahead) then runs.
    ///
    /// Not part of the stable API: the guarded implementations answer it, and
    /// [`Group::guard_first`](crate::router::group::Group::guard_first) reads
    /// it.
    #[doc(hidden)]
    const __GUARDED: bool = false;

    /// Runs the handler's guard from the head alone, ahead of the interceptors
    /// covering it, keeping what it admits for [`call`](Handler::call); the
    /// guard's rejection where it refuses.
    ///
    /// Not part of the stable API, as [`__GUARDED`](Handler::__GUARDED).
    #[doc(hidden)]
    fn __guard_ahead(
        parts: &mut Parts,
        context: &C,
    ) -> impl Future<Output = Option<Response>> + Send {
        let _ = (parts, context);
        std::future::ready(None)
    }
}
