//! Routing, grouping, and the path from code to description.
//!
//! [`Router`] is the root and lives here. [`endpoint`] holds one declared
//! operation and the builder a route attribute expands into, [`group`] a set of
//! operations sharing a prefix, [`operation`] the description being assembled,
//! [`policy`] the application-wide fallbacks, and [`service`] the built result.

pub mod endpoint;
pub mod group;
pub mod operation;
pub mod policy;
pub mod service;

use kynos_openapi::{Document, Info, SpecVersion, Violation};

use crate::{
    error::Result,
    middleware::{
        Interceptor, Observer,
        catch_panic::{Catch, PanicPolicy, Propagate},
        stack::{CompatibleStack, CompatibleWith, Cons},
    },
    router::{
        endpoint::set::IntoEndpoints,
        group::Group,
        operation::Tag,
        policy::{FallbackPolicy, TrailingSlashPolicy},
        service::Service,
    },
    security::SecurityScheme,
};

/// The root of an API.
///
/// `C` is the application context type — the dependency-injection container
/// every handler resolves its state from. A handler asking for something the
/// context does not provide is a compile error, not a runtime panic.
#[derive(Debug)]
pub struct Router<C, P = Propagate, I = ()> {
    // `fn() -> _` so that the parameters name a shape without deciding this
    // builder's auto traits: a router is `Send` because what it holds is, not
    // because `C` happens to be.
    //
    // `I` is the interceptors mounted here, as a type-level list. Nothing reads
    // it at run time -- the chain itself is erased -- but `intercept` and the
    // composition methods bound on it, which is what makes two colliding
    // interceptors a compile error rather than a build-time one.
    // The lint is measuring the three parameters the type genuinely
    // has; factoring them into an alias would hide the shape rather
    // than simplify it.
    #[allow(clippy::type_complexity)]
    _private: std::marker::PhantomData<fn() -> (C, P, I)>,
}

impl<C> Default for Router<C, Propagate, ()> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C> Router<C, Propagate, ()> {
    /// Creates an empty router.
    #[must_use]
    pub fn new() -> Self {
        todo!()
    }
}

impl<C, P: PanicPolicy, I> Router<C, P, I> {
    /// Converts panics from covered operations into documented 500 responses.
    ///
    /// The policy is carried in the router's type and resolved when the service
    /// is built. No recovery branch is installed when this method is not
    /// called.
    ///
    /// # Compile-time requirement
    ///
    /// The final binary must use `panic = "unwind"`. Selecting this policy in
    /// a `panic = "abort"` build is a compile-time error.
    ///
    /// ```no_run
    /// let router = kynos::Router::<()>::new().catch_panics();
    /// # let _ = router;
    /// ```
    #[must_use]
    pub fn catch_panics(self) -> Router<C, Catch> {
        const {
            assert!(
                cfg!(panic = "unwind"),
                "Kynos panic recovery requires `panic = \"unwind\"`; remove `catch_panics` or enable unwinding"
            );
        }
        todo!()
    }

    /// Sets the description's `info` block.
    #[must_use]
    pub fn info(self, info: Info) -> Self {
        let _ = info;
        todo!()
    }

    /// Declares a server providing this API.
    ///
    /// Never inferred from the bind address: the description states the public
    /// URL clients use, which is usually not the socket the process listens on.
    #[must_use]
    pub fn server(self, server: kynos_openapi::Server) -> Self {
        let _ = server;
        todo!()
    }

    /// Mounts operations at the router's root.
    #[must_use]
    pub fn mount<E: IntoEndpoints<C>>(self, endpoints: E) -> Self
    where
        E::Stacks: CompatibleStack<I, C>,
    {
        let () = <E::Stacks as CompatibleStack<I, C>>::CHECK;
        let _ = endpoints;
        todo!()
    }

    /// Mounts a group.
    #[must_use]
    pub fn group<GP: PanicPolicy, GI>(self, group: Group<C, GP, GI>) -> Self
    where
        GI: CompatibleStack<I, C>,
    {
        let () = <GI as CompatibleStack<I, C>>::CHECK;
        let _ = group;
        todo!()
    }

    /// Mounts another router beneath a path prefix.
    #[must_use]
    pub fn nest<NP: PanicPolicy, NI>(self, prefix: &'static str, router: Router<C, NP, NI>) -> Self
    where
        NI: CompatibleStack<I, C>,
    {
        let () = <NI as CompatibleStack<I, C>>::CHECK;
        let _ = (prefix, router);
        todo!()
    }

    /// Merges another router at the same level.
    #[must_use]
    pub fn merge<OP: PanicPolicy, OI>(self, router: Router<C, OP, OI>) -> Self
    where
        OI: CompatibleStack<I, C>,
    {
        let () = <OI as CompatibleStack<I, C>>::CHECK;
        let _ = router;
        todo!()
    }

    /// Declares a security scheme the API can use.
    #[must_use]
    pub fn security_scheme<S: SecurityScheme>(self) -> Self {
        todo!()
    }

    /// Registers tag metadata.
    #[must_use]
    pub fn tag<T: Tag>(self) -> Self {
        todo!()
    }

    /// Applies an interceptor to every operation in the router.
    #[must_use]
    pub fn intercept<N: Interceptor<C>>(self, interceptor: N) -> Router<C, P, Cons<N, I>>
    where
        C: Sync + 'static,
        I: CompatibleWith<N, C>,
    {
        // Forcing the const is what puts the error on this call rather than in
        // `middleware::stack`. Two interceptors adding one header, or
        // answering with one status, stop here.
        let () = <I as CompatibleWith<N, C>>::CHECK;
        let _ = interceptor;
        todo!()
    }

    /// Installs an observer.
    ///
    /// Observers see everything and change nothing, so they contribute nothing
    /// to the description. This is where request logging belongs.
    #[must_use]
    pub fn observe<O: Observer<C>>(self, observer: O) -> Self {
        let _ = observer;
        todo!()
    }

    /// Sets what happens when no route matches.
    ///
    /// Not a route: an unmatched path is outside the description entirely, and
    /// contributes no `paths` entry.
    #[must_use]
    pub fn not_found(self, policy: FallbackPolicy) -> Self {
        let _ = policy;
        todo!()
    }

    /// Sets what happens when a path matches but the method does not.
    ///
    /// The `Allow` header is derived from the operations actually declared on
    /// that path, so it cannot disagree with the description.
    #[must_use]
    pub fn method_not_allowed(self, policy: FallbackPolicy) -> Self {
        let _ = policy;
        todo!()
    }

    /// Sets the application-wide trailing-slash policy.
    ///
    /// Redirect mode only adds or removes the final slash to reach an exactly
    /// declared path. It never changes path casing or normalizes individual
    /// routes, and uses 308 so the request method and body are preserved.
    #[must_use]
    pub fn trailing_slashes(self, policy: TrailingSlashPolicy) -> Self {
        let _ = policy;
        todo!()
    }

    /// Turns unconstrained-schema warnings into build errors.
    ///
    /// [`Unchecked`](crate::schema::unchecked::Unchecked) is honest but weak. A team that
    /// wants no weak schemas at all can say so here.
    #[must_use]
    pub fn deny_unchecked_schemas(self) -> Self {
        todo!()
    }

    /// Checks the router without building it.
    ///
    /// Returns every violation, including warnings. Worth an integration test:
    /// it catches the mistakes that only show up across a whole API — a
    /// duplicated `operationId`, two paths that differ only in variable name, a
    /// security requirement naming a scheme nobody declared.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`](crate::Error::Invalid) if the router cannot be described at all.
    pub fn validate(&self) -> Result<Vec<Violation>> {
        todo!()
    }

    /// Produces the OpenAPI description.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`](crate::Error::Invalid) when validation finds an error-level
    /// violation, so a misleading description is never emitted.
    pub fn openapi(&self) -> Result<Document> {
        todo!()
    }

    /// Produces the description targeting a specific specification version.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`](crate::Error::Invalid) on a validation error, or if the API uses a
    /// construct `version` cannot express — a Server-Sent Events response
    /// requested as 3.1, say.
    pub fn openapi_as(&self, version: SpecVersion) -> Result<Document> {
        let _ = version;
        todo!()
    }

    /// Finalizes the router into something servable.
    ///
    /// This is where the structural checks run, so an API that cannot be
    /// described correctly fails at startup rather than at documentation time.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`](crate::Error::Invalid) with every violation found.
    pub fn build(self, context: C) -> Result<Service<C>>
    where
        C: Send + Sync + 'static,
    {
        let _ = context;
        todo!()
    }
}
