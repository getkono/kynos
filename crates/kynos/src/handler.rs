//! The bridge from an `async fn` to an operation.

use std::future::Future;

use crate::{
    extract::describe::Describe,
    http::{Request, Response},
    router::OperationCx,
    schema::Registry,
};

/// An `async fn` usable as an operation handler.
///
/// Implemented for functions of up to sixteen arguments where each argument is
/// a describable request input or a context-derived value, and the return type
/// describes its responses.
///
/// The bounds are the whole enforcement mechanism. An argument that cannot
/// implement [`Describe`] — a raw request, a whole header map, an untyped body
/// — has no way into a handler signature, and a return type that cannot
/// implement [`Responses`](crate::response::Responses) has no way out.
///
/// `A` is the argument tuple; it exists only so that the implementations do not
/// overlap, and is inferred at every call site.
pub trait Handler<C, A = ()>: Clone + Send + Sync + 'static {
    /// Runs the handler.
    fn call(self, request: Request, context: C) -> impl Future<Output = Response> + Send;

    /// Describes the handler's inputs and outputs.
    ///
    /// Delegates to each argument's [`Describe`] implementation and to the
    /// return type's [`Responses`](crate::response::Responses), which is why
    /// there is no attribute DSL restating the signature — and so nothing that
    /// can drift from it.
    fn describe(registry: &mut Registry) -> kynos_openapi::Operation;
}

/// The set of statuses an operation can produce, gathered from its parts.
///
/// An operation's `responses` is the union of its return type's responses and
/// the rejections of each of its inputs. That second half is what other
/// frameworks omit: a handler taking `Json<T>` can always answer 400 and 415,
/// whether or not anyone remembered to write that down.
#[derive(Debug, Default)]
pub struct ResponseSet {
    _private: (),
}

impl ResponseSet {
    /// Creates an empty set.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }

    /// Merges in the responses a handler input's rejection can produce.
    pub fn merge(&mut self, responses: kynos_openapi::Responses) {
        let _ = responses;
        todo!()
    }

    /// Consumes the set, yielding the operation's `responses`.
    #[must_use]
    pub fn into_responses(self) -> kynos_openapi::Responses {
        todo!()
    }
}

/// Describes a handler argument into an operation.
///
/// Called once per argument by the generated [`Handler`] implementations.
pub fn describe_input<T: Describe>(operation: &mut OperationCx<'_>) {
    T::describe(operation);
}
