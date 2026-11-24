//! Building an endpoint at runtime rather than with a route attribute.
//!
//! The setters assembling one are here; what it does once mounted, describing
//! its operation and serving it, is in the private `serve`.

mod serve;

use std::{marker::PhantomData, sync::Arc};

use kynos_openapi::{Method, PathTemplate};

use crate::{
    handler::Handler,
    middleware::{
        Interceptor,
        catch_panic::{Catch, PanicPolicy, Propagate},
        erased::ErasedInterceptor,
        stack::{self, CompatibleWith, Cons},
    },
    router::{
        endpoint::set::{Endpoints, IntoEndpoints},
        install,
        operation::{DeclaredTag, Tag},
    },
};

/// Builds an endpoint at runtime, rather than with a route attribute.
///
/// The attribute macros expand into this. Reach for it directly only when the
/// set of routes is not known at compile time; the attribute checks things this
/// cannot, notably that a handler's path parameters match its path template.
///
/// ```no_run
/// # use kynos::{openapi, router::endpoint::builder::EndpointBuilder};
/// async fn health() -> kynos::response::status::NoContent {
///     kynos::response::status::NoContent
/// }
///
/// let endpoint = EndpointBuilder::new(
///     openapi::Method::Get,
///     openapi::PathTemplate::parse("/health").expect("valid path"),
///     health,
/// )
/// .intercept(kynos::middleware::request_id::RequestId::new());
/// let router = kynos::Router::<()>::new().mount(endpoint);
/// # let _ = router;
/// ```
pub struct EndpointBuilder<C, H, A, P = Propagate, I = ()> {
    method: Method,
    path: PathTemplate,
    handler: H,
    operation_id: Option<Named>,
    summary: Option<&'static str>,
    description: Option<&'static str>,
    tags: Vec<DeclaredTag>,
    deprecated: bool,
    public: bool,
    /// The `operationId` of the `HEAD` this `GET` describes, if it does.
    head: Option<Named>,
    /// Every interceptor this operation was given, outermost first.
    written: Vec<Arc<dyn ErasedInterceptor<C>>>,
    /// Those of `written` that cover this operation, which run and are
    /// declared: the rest cover only operations whose body something reads.
    interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,

    // `fn() -> _` keeps the parameters out of this builder's auto traits. `I`
    // is this operation's interceptor list, which `mount` checks.
    #[allow(clippy::type_complexity)]
    _private: PhantomData<fn() -> (C, H, A, P, I)>,
}

/// An `operationId`, and whether the application wrote it or a route attribute
/// derived it, which only the router's
/// [`OperationIdCase`](crate::router::operation::OperationIdCase) recases.
#[derive(Clone, Copy, Debug)]
enum Named {
    Written(&'static str),
    Derived(&'static str),
}

impl<C, H, A, P, I> std::fmt::Debug for EndpointBuilder<C, H, A, P, I> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EndpointBuilder")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("operation_id", &self.operation_id)
            .field("interceptors", &self.interceptors.len())
            .finish_non_exhaustive()
    }
}

impl<C, H: Handler<C, A>, A> EndpointBuilder<C, H, A, Propagate, ()> {
    /// Begins an endpoint for `handler`.
    #[must_use]
    pub fn new(method: Method, path: PathTemplate, handler: H) -> Self {
        Self::with_policy(method, path, handler)
    }
}

impl<C, H: Handler<C, A>, A, P: PanicPolicy, I> EndpointBuilder<C, H, A, P, I> {
    /// Begins an endpoint whose panic policy is already decided, as a route
    /// attribute knows it.
    pub(crate) fn with_policy(method: Method, path: PathTemplate, handler: H) -> Self {
        Self {
            method,
            path,
            handler,
            operation_id: None,
            summary: None,
            description: None,
            tags: Vec::new(),
            deprecated: false,
            public: false,
            head: None,
            written: Vec::new(),
            interceptors: Vec::new(),
            _private: PhantomData,
        }
    }

    /// Carries every field across a change of type parameter.
    fn retype<Q, J>(self) -> EndpointBuilder<C, H, A, Q, J> {
        EndpointBuilder {
            method: self.method,
            path: self.path,
            handler: self.handler,
            operation_id: self.operation_id,
            summary: self.summary,
            description: self.description,
            tags: self.tags,
            deprecated: self.deprecated,
            public: self.public,
            head: self.head,
            written: self.written,
            interceptors: self.interceptors,
            _private: PhantomData,
        }
    }

    /// Converts panics from this operation into a documented 500 response.
    ///
    /// Extraction and handler execution are covered. The policy is carried in
    /// the endpoint's type, so an endpoint that does not select it installs no
    /// recovery branch. A panic recovered here reaches the router's observers
    /// through [`Observer::on_panic`](crate::middleware::Observer::on_panic),
    /// as one recovered at router or group scope does.
    ///
    /// Only the panic policy changes: the result keeps the interceptor list
    /// `I`, so an interceptor added after this is still checked against the
    /// ones added before it.
    ///
    /// # Compile-time requirement
    ///
    /// The final binary must use `panic = "unwind"`. Selecting this policy in
    /// a `panic = "abort"` build is a compile-time error.
    ///
    /// ```no_run
    /// # use kynos::{openapi, router::endpoint::builder::EndpointBuilder};
    /// async fn health() -> kynos::response::status::NoContent {
    ///     kynos::response::status::NoContent
    /// }
    ///
    /// let endpoint = EndpointBuilder::<(), _, _>::new(
    ///     openapi::Method::Get,
    ///     openapi::PathTemplate::parse("/health").expect("valid path"),
    ///     health,
    /// )
    /// .catch_panics();
    /// # let _ = endpoint;
    /// ```
    #[must_use]
    pub fn catch_panics(self) -> EndpointBuilder<C, H, A, Catch, I> {
        const { install::require_unwind() }
        self.retype()
    }

    /// Sets the operation identifier, published exactly as written.
    ///
    /// A route attribute defaults it to the handler's own name, which the
    /// router's [`OperationIdCase`](crate::router::operation::OperationIdCase)
    /// may recase. Override it only to keep a generated client's method name
    /// stable across a refactor.
    #[must_use]
    pub fn operation_id(mut self, id: &'static str) -> Self {
        self.operation_id = Some(Named::Written(id));
        self
    }

    /// Sets an operation identifier a route attribute derived, which the
    /// router's case recases.
    pub(crate) fn derived_operation_id(mut self, id: &'static str) -> Self {
        self.operation_id = Some(Named::Derived(id));
        self
    }

    /// Sets the operation summary.
    #[must_use]
    pub fn summary(mut self, summary: &'static str) -> Self {
        self.summary = Some(summary);
        self
    }

    /// Sets the operation description.
    #[must_use]
    pub fn description(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }

    /// Tags the operation.
    ///
    /// The tag's metadata reaches the document's own `tags` with it, so an
    /// operation tagged only here is not filed under a heading nothing
    /// declares.
    #[must_use]
    pub fn tag<T: Tag>(mut self) -> Self {
        self.tags.push(DeclaredTag::of::<T>());
        self
    }

    /// Tags the operation with a tag already resolved to a value, as
    /// `from_meta` holds it.
    pub(crate) fn with_tag(mut self, tag: DeclaredTag) -> Self {
        self.tags.push(tag);
        self
    }

    /// Marks the operation deprecated.
    #[must_use]
    pub fn deprecated(mut self) -> Self {
        self.deprecated = true;
        self
    }

    /// Declares that the operation takes no guard on purpose.
    ///
    /// A [`Group`](crate::router::group::Group) whose operations take a guard
    /// refuses to build around one that takes none, since that is what a
    /// forgotten guard argument looks like; this is how an operation that is
    /// meant to be reachable without a credential, such as the one issuing
    /// it, says so. It changes nothing else: the description and the request
    /// path are the same with it and without it. `#[kynos::public]` is the
    /// route attribute's spelling.
    ///
    /// Only for a handler that takes no guard. On one that does, it fails to
    /// compile, since an operation cannot be both:
    ///
    /// ```compile_fail,E0080
    /// # use kynos::{
    /// #     error::rejection::AuthRejection, openapi,
    /// #     router::endpoint::builder::EndpointBuilder,
    /// #     security::{Authenticates, Authenticator, Head, auth::Auth, schemes::Bearer},
    /// #     security::carrier::{BearerToken, presented::Presented},
    /// # };
    /// # struct Tokens;
    /// # impl<C: Sync> Authenticator<Bearer, C> for Tokens {
    /// #     async fn authenticate(
    /// #         &self, _: Presented<BearerToken>, _: Head<'_>, _: &C,
    /// #     ) -> Result<String, AuthRejection> {
    /// #         Err(AuthRejection::unauthenticated())
    /// #     }
    /// # }
    /// # struct App;
    /// # impl Authenticates<Bearer> for App {
    /// #     type Authenticator = Tokens;
    /// #     fn authenticator(&self) -> &Tokens { &Tokens }
    /// # }
    /// async fn revoke(caller: Auth<Bearer>) -> kynos::response::status::NoContent {
    ///     kynos::response::status::NoContent
    /// }
    ///
    /// let endpoint = EndpointBuilder::<App, _, _>::new(
    ///     openapi::Method::Delete,
    ///     openapi::PathTemplate::parse("/sessions/current").expect("valid path"),
    ///     revoke,
    /// )
    /// .public();
    /// # let _ = endpoint;
    /// ```
    #[must_use]
    pub fn public(self) -> Self {
        const {
            assert!(
                !H::__GUARDED,
                "`public` declares that an operation takes no guard, and this handler takes one; \
                 remove `public`, or the guard argument"
            );
        };
        self.declared_public()
    }

    /// [`public`](EndpointBuilder::public) without its check, for `from_meta`,
    /// which refuses a guarded handler in the attribute's words instead.
    pub(crate) fn declared_public(mut self) -> Self {
        self.public = true;
        self
    }

    /// Describes the `HEAD` this `GET` already answers, as an operation of its
    /// own named `operation_id`.
    ///
    /// A `GET` answers `HEAD` on its path whether or not this is called (RFC
    /// 9110 section 9.3.2); this only puts that `HEAD` in the description, so
    /// a client generated from it can call it. The `HEAD` is the `GET`'s
    /// operation, its parameters, security and every response's headers
    /// included, with no response carrying content and no range handling,
    /// which is defined for `GET` alone (RFC 9110 section 14.2): no `Range` or
    /// `If-Range` parameter and no 206 or 416. It runs the `GET`'s
    /// handler under the same interceptors, which see it as `operation_id`,
    /// published exactly as written.
    /// `#[kynos::get(.., head)]` is the route attribute's spelling.
    ///
    /// Building refuses it on anything but a `GET`, as
    /// [`RouterError::HeadWithoutGet`](crate::router::error::RouterError::HeadWithoutGet),
    /// and on a path that also declares a `HEAD` operation, as
    /// [`RouterError::HeadDescribedTwice`](crate::router::error::RouterError::HeadDescribedTwice).
    ///
    /// ```no_run
    /// # use kynos::{openapi, router::endpoint::builder::EndpointBuilder};
    /// async fn get_report() -> kynos::response::status::NoContent {
    ///     kynos::response::status::NoContent
    /// }
    ///
    /// let endpoint = EndpointBuilder::<(), _, _>::new(
    ///     openapi::Method::Get,
    ///     openapi::PathTemplate::parse("/report").expect("valid path"),
    ///     get_report,
    /// )
    /// .operation_id("get_report")
    /// .describe_head("head_report");
    /// # let _ = endpoint;
    /// ```
    #[must_use]
    pub fn describe_head(mut self, operation_id: &'static str) -> Self {
        self.head = Some(Named::Written(operation_id));
        self
    }

    /// [`describe_head`](EndpointBuilder::describe_head) under a name a bare
    /// `head` derived, which the router's case recases.
    pub(crate) fn describe_derived_head(mut self, operation_id: &'static str) -> Self {
        self.head = Some(Named::Derived(operation_id));
        self
    }

    /// Applies an interceptor to this operation only.
    ///
    /// The interceptor's contribution is merged into this endpoint's
    /// description.
    ///
    /// The first call is the outermost of this endpoint's own, and every one of
    /// them is innermost of all: a router's and a group's both wrap them. See
    /// [the module's ordering rule](crate::middleware#the-order-a-chain-runs-in).
    #[must_use]
    pub fn intercept<N: Interceptor<C>>(
        self,
        interceptor: N,
    ) -> EndpointBuilder<C, H, A, P, Cons<N, I>>
    where
        C: Sync + 'static,
        I: CompatibleWith<N, C>,
    {
        let () = <I as CompatibleWith<N, C>>::CHECK;
        const { stack::unscoped::<C, N>() };

        let mut builder: EndpointBuilder<C, H, A, P, Cons<N, I>> = self.retype();
        // Every one stays in the type, so the collision check reads what was
        // written; each newest is innermost, so what covers is walked again.
        builder.written.push(Arc::new(interceptor));
        builder.interceptors = crate::router::covered(builder.written.clone(), H::__READS_BODY);
        builder
    }
}

impl<C, H, A, P, I> IntoEndpoints<C> for EndpointBuilder<C, H, A, P, I>
where
    C: Send + Sync + 'static,
    H: Handler<C, A>,
    // Only `'static`: `A` is a phantom argument shape, never a value.
    A: 'static,
    P: PanicPolicy,
    I: 'static,
{
    /// Whatever `intercept` accumulated on this one operation.
    type Stacks = I;

    fn into_endpoints(self, sink: &mut Endpoints<C>) {
        sink.push(self);
    }
}
