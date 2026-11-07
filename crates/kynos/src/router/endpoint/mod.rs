//! One declared operation: what a route attribute produces, and the builder it
//! expands into.
//!
//! [`Endpoint`] and its erasure live here; [`meta`] holds the compile-time
//! facts a route attribute knows, [`set`] the collection a router mounts, and
//! [`builder`] the runtime alternative to the attribute.

pub mod builder;
pub mod meta;
pub mod set;

use std::{convert::Infallible, future::Future, pin::Pin, sync::Arc};

use kynos_openapi::{Method, PathTemplate};

use crate::{
    http::{Request, Response},
    middleware::{Continued, Interceptor, MiddlewareError, Next, erased::ErasedInterceptor},
    router::operation::OperationCx,
};

/// A declared API operation.
///
/// Implemented by [`EndpointBuilder`](builder::EndpointBuilder), which is also
/// what a route attribute's operation becomes: the attribute emits a braced
/// zero-sized type implementing [`EndpointMeta`](meta::EndpointMeta) under the
/// handler's name, which occupies only the type namespace and so does not
/// shadow the function, and `routes![get_user]` builds an `EndpointBuilder`
/// from the two.
///
/// Prefer the attribute over the builder: it takes the doc comment as the
/// operation's summary and description, and checks the path template against
/// the handler's parameters at compile time.
pub trait Endpoint<C>: Send + Sync + 'static {
    /// The HTTP method.
    fn method(&self) -> Method;

    /// The path template, relative to any enclosing group.
    fn path(&self) -> &PathTemplate;

    /// Describes this operation, registering any schemas it needs.
    fn describe(&self, operation: &mut OperationCx<'_>);

    /// Handles a request.
    fn call(&self, request: Request, context: &C) -> impl Future<Output = Response> + Send;

    /// The configuration conflict an interceptor this operation holds itself
    /// carries, which the router refuses to build over.
    ///
    /// Not part of the stable API: [`EndpointBuilder`](builder::EndpointBuilder)
    /// answers it from what `intercept` added, and an endpoint holding no
    /// interceptor of its own has nothing to answer.
    #[doc(hidden)]
    fn __unhonourable_interceptor(&self) -> Option<MiddlewareError> {
        None
    }

    /// Whether this operation's handler takes a guard, which
    /// [`__guard_ahead`](Endpoint::__guard_ahead) then runs.
    ///
    /// Not part of the stable API: [`EndpointBuilder`](builder::EndpointBuilder)
    /// answers it from its handler, and an endpoint holding no guard has
    /// nothing to answer.
    #[doc(hidden)]
    fn __guarded(&self) -> bool {
        false
    }

    /// Runs this operation's guard from the request head, ahead of the
    /// interceptors a [`guard_first`](crate::router::group::Group::guard_first)
    /// scope puts it before, handing the request on with what the guard
    /// admitted or answering with its rejection.
    ///
    /// Not part of the stable API, as [`__guarded`](Endpoint::__guarded).
    #[doc(hidden)]
    fn __guard_ahead(
        &self,
        request: Request,
        context: &C,
    ) -> impl Future<Output = Result<Request, Response>> + Send {
        let _ = context;
        std::future::ready(Ok(request))
    }
}

/// An operation's guard, run as an interceptor at the point of the chain a
/// [`guard_first`](crate::router::group::Group::guard_first) scope chose.
///
/// Installed only when the service is built, and reading, adding and
/// short-circuiting with nothing it declares, so it contributes nothing to the
/// description: the guard argument has declared its refusals already. Its
/// refusal is the guard's response, handed through as the chain's own.
struct GuardAhead<E>(Arc<E>);

impl<C, E> Interceptor<C> for GuardAhead<E>
where
    C: Send + Sync + 'static,
    E: Endpoint<C>,
{
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        reads: (),
        context: &C,
        next: Next<'_, C>,
    ) -> Result<Continued<()>, Infallible> {
        let () = reads;
        Ok(match self.0.__guard_ahead(request, context).await {
            Ok(admitted) => next.run(admitted).await,
            Err(refused) => Continued::new(refused),
        })
    }
}

/// The object-safe form of [`Endpoint`], so a router can hold a heterogeneous
/// set of them.
pub(crate) trait DynEndpoint<C>: Send + Sync + 'static {
    fn method(&self) -> Method;

    fn path(&self) -> &PathTemplate;

    fn describe(&self, operation: &mut OperationCx<'_>);

    fn call<'a>(
        &'a self,
        request: Request,
        context: &'a C,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>;

    fn unhonourable_interceptor(&self) -> Option<MiddlewareError>;

    /// This operation's guard as an interceptor, where its handler takes one.
    fn guard_ahead(self: Arc<Self>) -> Option<Arc<dyn ErasedInterceptor<C>>>;
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

    fn unhonourable_interceptor(&self) -> Option<MiddlewareError> {
        Endpoint::__unhonourable_interceptor(self)
    }

    fn guard_ahead(self: Arc<Self>) -> Option<Arc<dyn ErasedInterceptor<C>>> {
        // Concrete, so the one box the interceptor's future takes holds the
        // guard's own.
        Endpoint::__guarded(&*self).then(|| Arc::new(GuardAhead(self)) as _)
    }
}

/// A stable, readable identifier for one served path, for operations no
/// handler function names. Derived from the path, so it does not move when a
/// sibling is added.
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
        // Generators turn an `operationId` into a function name.
        if character.is_ascii_alphanumeric() {
            id.push(character);
        } else {
            id.push('_');
        }
    }
    id
}
