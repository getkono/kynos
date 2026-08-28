//! One declared operation: what a route attribute produces, and the builder it
//! expands into.
//!
//! [`Endpoint`] and its erasure live here; [`meta`] holds the compile-time
//! facts a route attribute knows, [`set`] the collection a router mounts, and
//! [`builder`] the runtime alternative to the attribute.

pub mod builder;
pub mod meta;
pub mod set;

use std::{future::Future, pin::Pin};

use kynos_openapi::{Method, PathTemplate};

use crate::{
    http::{Request, Response},
    router::operation::OperationCx,
};

/// A declared API operation.
///
/// Produced by the route attribute macros, which expand a handler function into
/// a zero-sized type implementing this trait. The type shadows the function
/// name, so `routes![get_user]` refers to the operation rather than the `fn`.
///
/// The builder form is public and supported for routes composed at runtime, but
/// the attribute is the recommended way: it takes the doc comment as the
/// operation's summary and description, and it can check the path template
/// against the handler's parameters at compile time, which the builder cannot.
pub trait Endpoint<C>: Send + Sync + 'static {
    /// The HTTP method.
    fn method(&self) -> Method;

    /// The path template, relative to any enclosing group.
    fn path(&self) -> &PathTemplate;

    /// Describes this operation, registering any schemas it needs.
    fn describe(&self, operation: &mut OperationCx<'_>);

    /// Handles a request.
    fn call(&self, request: Request, context: &C) -> impl Future<Output = Response> + Send;
}

/// The object-safe form of [`Endpoint`], so a router can hold a heterogeneous
/// set of them.
///
/// Private: boxing the future is how erasure is paid for, and no public
/// signature names a boxed future.
pub(crate) trait DynEndpoint<C>: Send + Sync + 'static {
    fn method(&self) -> Method;

    fn path(&self) -> &PathTemplate;

    fn describe(&self, operation: &mut OperationCx<'_>);

    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;
}

impl<C: Send + Sync + 'static, E: Endpoint<C>> DynEndpoint<C> for E {
    fn method(&self) -> Method {
        Endpoint::method(self)
    }

    fn path(&self) -> &PathTemplate {
        Endpoint::path(self)
    }

    fn describe(&self, operation: &mut OperationCx<'_>) {
        Endpoint::describe(self, operation);
    }

    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(Endpoint::call(self, request, context))
    }
}

/// A stable, readable identifier for one served path.
///
/// Derived from the path rather than counted, so two sets mounted in one router
/// collide only where they genuinely serve the same path -- and so the id does
/// not move when a sibling is added beside it.
///
/// Shared by the modules that register operations no handler function named,
/// and so have no identifier to take from one. Gated on exactly those, because
/// a build with neither reaches it from nowhere.
#[cfg(any(feature = "assets", feature = "docs"))]
pub(crate) fn operation_id(prefix: &str, path: &str) -> String {
    let mut id = String::with_capacity(prefix.len() + path.len() + 1);
    id.push_str(prefix);

    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        id.push_str("_index");
        return id;
    }

    id.push('_');
    for character in trimmed.chars() {
        // An `operationId` is a token a generator turns into a function name,
        // so anything that is not one becomes `_`.
        if character.is_ascii_alphanumeric() {
            id.push(character);
        } else {
            id.push('_');
        }
    }
    id
}
