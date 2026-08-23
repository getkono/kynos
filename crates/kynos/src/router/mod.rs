//! Routing, grouping, and the path from code to description.
//!
//! [`Router`] is the root and lives here. [`endpoint`] holds one declared
//! operation and the builder a route attribute expands into, [`group`] a set of
//! operations sharing a prefix, [`operation`] the description being assembled,
//! [`policy`] the application-wide fallbacks, and [`service`] the built result.

#[cfg(feature = "assets")]
pub mod assets;
pub mod endpoint;
pub mod group;
pub mod operation;
pub mod policy;
pub mod service;

// The runtime match table and what one request does to it. Private because it
// declares no item a user could name: everything in it is machinery `build`
// assembles and `Service` drives.
pub(crate) mod dispatch;

use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use kynos_openapi::{
    Document, Info, Paths, Severity, SpecError, SpecVersion, Violation,
    model::paths::item::PathItem,
};

use crate::{
    error::{Error, Result},
    middleware::{
        Interceptor, Observer,
        catch_panic::{Catch, PanicPolicy, Propagate},
        erased::ErasedInterceptor,
        stack::{CompatibleStack, CompatibleWith, Cons},
    },
    response::short_circuit_mismatch,
    router::{
        dispatch::{Dispatch, EndpointTerminal, PathEntry},
        endpoint::{DynEndpoint, set::IntoEndpoints},
        group::Group,
        operation::{OperationCx, Route, Tag},
        policy::{FallbackPolicy, TrailingSlashPolicy},
        service::Service,
    },
    schema::registry::Registry,
    security::SecurityScheme,
};

/// A `ShortCircuit`'s two halves, compared once the registry exists.
///
/// A function pointer rather than a trait object: the comparison is a fact
/// about a type, and the type is known where the interceptor is mounted but
/// erased everywhere after.
pub(crate) type ShortCircuitCheck = fn(&mut Registry) -> Option<SpecError>;

/// One endpoint, plus what the scopes it passed through contributed to it.
pub(crate) struct Mounted<C> {
    endpoint: Arc<dyn DynEndpoint<C>>,
    /// The `paths` key, with every enclosing prefix already applied.
    path: kynos_openapi::PathTemplate,
    /// Group- and nested-router interceptors, outermost first. The router's own
    /// are held separately, because `intercept` may be called after `mount`.
    interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,
    tags: Vec<&'static str>,
    catch_panics: bool,
    /// Undescribed layers an enclosing scope wrapped this operation in,
    /// outermost first. `pub(crate)` because `unchecked` reads it back.
    #[cfg(feature = "unchecked")]
    pub(crate) unchecked_layers: Vec<Arc<dyn crate::unchecked::ErasedLayer>>,
}

/// The root of an API.
///
/// `C` is the application context type: the application's own struct, which
/// every handler resolves its state from. It is a type rather than a
/// container — nothing is registered into it and nothing is looked up — so a
/// handler asking for something the context does not provide is a compile
/// error rather than a runtime panic.
pub struct Router<C, P = Propagate, I = ()> {
    pub(crate) mounted: Vec<Mounted<C>>,
    pub(crate) interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,
    pub(crate) short_circuit_checks: Vec<ShortCircuitCheck>,
    pub(crate) observers: Vec<Arc<dyn Observer<C>>>,
    pub(crate) info: Option<Info>,
    pub(crate) servers: Vec<kynos_openapi::Server>,
    pub(crate) tags: Vec<&'static str>,
    pub(crate) tag_metadata: Vec<kynos_openapi::Tag>,
    pub(crate) security_schemes: Vec<(&'static str, kynos_openapi::SecurityScheme)>,
    /// Problems found while mounting, which the fluent methods cannot return.
    pub(crate) violations: Vec<Violation>,
    pub(crate) not_found: FallbackPolicy,
    pub(crate) method_not_allowed: FallbackPolicy,
    pub(crate) trailing_slashes: TrailingSlashPolicy,
    pub(crate) deny_unchecked_schemas: bool,
    /// The waivers taken here: routes no template expresses, and layers whose
    /// effect nothing declares.
    #[cfg(feature = "unchecked")]
    pub(crate) unchecked: crate::unchecked::Unchecked<C>,

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
    _private: PhantomData<fn() -> (C, P, I)>,
}

impl<C, P, I> std::fmt::Debug for Router<C, P, I> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Router")
            .field("operations", &self.mounted.len())
            .field("interceptors", &self.interceptors.len())
            .field("observers", &self.observers.len())
            .finish_non_exhaustive()
    }
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
        Self::empty()
    }
}

impl<C, P, I> Router<C, P, I> {
    /// The empty router at any parameterization.
    fn empty() -> Self {
        Self {
            mounted: Vec::new(),
            interceptors: Vec::new(),
            short_circuit_checks: Vec::new(),
            observers: Vec::new(),
            info: None,
            servers: Vec::new(),
            tags: Vec::new(),
            tag_metadata: Vec::new(),
            security_schemes: Vec::new(),
            violations: Vec::new(),
            not_found: FallbackPolicy::default(),
            method_not_allowed: FallbackPolicy::default(),
            trailing_slashes: TrailingSlashPolicy::default(),
            deny_unchecked_schemas: false,
            #[cfg(feature = "unchecked")]
            unchecked: crate::unchecked::Unchecked::default(),
            _private: PhantomData,
        }
    }

    /// Carries every field across a change of type parameter.
    ///
    /// `catch_panics` and `intercept` change what the type says without
    /// changing what the value holds, so this is a field-by-field move rather
    /// than a new router.
    fn retype<Q, J>(self) -> Router<C, Q, J> {
        Router {
            mounted: self.mounted,
            interceptors: self.interceptors,
            short_circuit_checks: self.short_circuit_checks,
            observers: self.observers,
            info: self.info,
            servers: self.servers,
            tags: self.tags,
            tag_metadata: self.tag_metadata,
            security_schemes: self.security_schemes,
            violations: self.violations,
            not_found: self.not_found,
            method_not_allowed: self.method_not_allowed,
            trailing_slashes: self.trailing_slashes,
            deny_unchecked_schemas: self.deny_unchecked_schemas,
            #[cfg(feature = "unchecked")]
            unchecked: self.unchecked,
            _private: PhantomData,
        }
    }

    /// Takes in a set of endpoints under `prefix`, recording what the enclosing
    /// scope contributes to each.
    fn absorb(
        &mut self,
        endpoints: Vec<Arc<dyn DynEndpoint<C>>>,
        prefix: &str,
        tags: &[&'static str],
        interceptors: &[Arc<dyn ErasedInterceptor<C>>],
        catch_panics: bool,
    ) where
        C: 'static,
    {
        for endpoint in endpoints {
            let path = match endpoint.path().with_prefix(prefix) {
                Ok(path) => path,
                Err(reason) => {
                    let template = endpoint.path().as_str().to_owned();
                    self.violations.push(error_at(
                        format!("#/paths/{}", pointer_token(&template)),
                        SpecError::InvalidPathTemplate { template, reason },
                    ));
                    continue;
                }
            };

            if let Some(error) = unroutable(&path) {
                self.violations.push(error_at(
                    format!("#/paths/{}", pointer_token(path.as_str())),
                    error,
                ));
                continue;
            }

            self.mounted.push(Mounted {
                endpoint,
                path,
                interceptors: interceptors.to_vec(),
                tags: tags.to_vec(),
                catch_panics,
                #[cfg(feature = "unchecked")]
                unchecked_layers: Vec::new(),
            });
        }
    }

    /// Takes in everything another router holds, under `prefix`.
    fn absorb_router<Q, J>(&mut self, other: Router<C, Q, J>, prefix: &str, catch_panics: bool)
    where
        C: 'static,
    {
        for mut mounted in other.mounted {
            match mounted.path.with_prefix(prefix) {
                Ok(path) => mounted.path = path,
                Err(reason) => {
                    let template = mounted.path.as_str().to_owned();
                    self.violations.push(error_at(
                        format!("#/paths/{}", pointer_token(&template)),
                        SpecError::InvalidPathTemplate { template, reason },
                    ));
                    continue;
                }
            }

            if let Some(error) = unroutable(&mounted.path) {
                self.violations.push(error_at(
                    format!("#/paths/{}", pointer_token(mounted.path.as_str())),
                    error,
                ));
                continue;
            }

            // The absorbed router's own interceptors and tags covered exactly
            // these operations, so they become part of what each carries rather
            // than of what this router applies to everything.
            let mut interceptors = other.interceptors.clone();
            interceptors.append(&mut mounted.interceptors);
            mounted.interceptors = interceptors;

            let mut tags = other.tags.clone();
            tags.append(&mut mounted.tags);
            mounted.tags = tags;

            #[cfg(feature = "unchecked")]
            {
                let mut layers = other.unchecked.layers.clone();
                layers.append(&mut mounted.unchecked_layers);
                mounted.unchecked_layers = layers;
            }

            mounted.catch_panics |= catch_panics;
            self.mounted.push(mounted);
        }

        // An observer sees whole requests rather than operations, so an
        // absorbed one widens to this router rather than being dropped.
        self.observers.extend(other.observers);
        self.short_circuit_checks.extend(other.short_circuit_checks);
        self.servers.extend(other.servers);
        self.tag_metadata.extend(other.tag_metadata);
        self.security_schemes.extend(other.security_schemes);
        self.violations.extend(other.violations);
        self.deny_unchecked_schemas |= other.deny_unchecked_schemas;
        #[cfg(feature = "unchecked")]
        self.unchecked.absorb(other.unchecked, prefix);
        if self.info.is_none() {
            self.info = other.info;
        }
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
        self.retype()
    }

    /// Sets the description's `info` block.
    #[must_use]
    pub fn info(mut self, info: Info) -> Self {
        self.info = Some(info);
        self
    }

    /// Declares a server providing this API.
    ///
    /// Never inferred from the bind address: the description states the public
    /// URL clients use, which is usually not the socket the process listens on.
    #[must_use]
    pub fn server(mut self, server: kynos_openapi::Server) -> Self {
        self.servers.push(server);
        self
    }

    /// Mounts operations at the router's root.
    #[must_use]
    pub fn mount<E: IntoEndpoints<C>>(mut self, endpoints: E) -> Self
    where
        C: 'static,
        E::Stacks: CompatibleStack<I, C>,
    {
        let () = <E::Stacks as CompatibleStack<I, C>>::CHECK;

        let mut sink = endpoint::set::Endpoints::new();
        endpoints.into_endpoints(&mut sink);
        self.absorb(sink.into_inner(), "", &[], &[], false);
        self
    }

    /// Mounts a group.
    #[must_use]
    pub fn group<GP: PanicPolicy, GI>(mut self, group: Group<C, GP, GI>) -> Self
    where
        C: 'static,
        GI: CompatibleStack<I, C>,
    {
        let () = <GI as CompatibleStack<I, C>>::CHECK;

        let group = group.into_parts();
        self.violations.extend(group.violations);
        self.tag_metadata.extend(group.tag_metadata);
        self.short_circuit_checks.extend(group.short_circuit_checks);

        #[cfg(feature = "unchecked")]
        let first = self.mounted.len();

        self.absorb(
            group.endpoints,
            &group.prefix,
            &group.tags,
            &group.interceptors,
            catches::<GP>(),
        );

        // A group's layers cover the group's operations and nothing else, which
        // is why they land on each absorbed endpoint rather than on the router.
        #[cfg(feature = "unchecked")]
        for mounted in &mut self.mounted[first..] {
            mounted.unchecked_layers.clone_from(&group.unchecked_layers);
        }

        self
    }

    /// Mounts another router beneath a path prefix.
    #[must_use]
    pub fn nest<NP: PanicPolicy, NI>(
        mut self,
        prefix: &'static str,
        router: Router<C, NP, NI>,
    ) -> Self
    where
        C: 'static,
        NI: CompatibleStack<I, C>,
    {
        let () = <NI as CompatibleStack<I, C>>::CHECK;

        self.absorb_router(router, prefix, catches::<NP>());
        self
    }

    /// Merges another router at the same level.
    #[must_use]
    pub fn merge<OP: PanicPolicy, OI>(mut self, router: Router<C, OP, OI>) -> Self
    where
        C: 'static,
        OI: CompatibleStack<I, C>,
    {
        let () = <OI as CompatibleStack<I, C>>::CHECK;

        self.absorb_router(router, "", catches::<OP>());
        self
    }

    /// Declares a security scheme the API can use.
    #[must_use]
    pub fn security_scheme<S: SecurityScheme>(mut self) -> Self {
        self.security_schemes.push((S::NAME, S::describe()));
        self
    }

    /// Registers tag metadata.
    #[must_use]
    pub fn tag<T: Tag>(mut self) -> Self {
        self.tags.push(T::NAME);
        self.tag_metadata.push(T::metadata());
        self
    }

    /// Applies an interceptor to every operation in the router.
    ///
    /// The first call is the outermost interceptor; see
    /// [the module's ordering rule](crate::middleware#the-order-a-chain-runs-in).
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

        let mut router: Router<C, P, Cons<N, I>> = self.retype();
        router.interceptors.push(Arc::new(interceptor));
        router
            .short_circuit_checks
            .push(short_circuit_mismatch::<N::Short>);
        router
    }

    /// Installs an observer.
    ///
    /// Observers see everything and change nothing, so they contribute nothing
    /// to the description. This is where request logging belongs.
    #[must_use]
    pub fn observe<O: Observer<C>>(mut self, observer: O) -> Self {
        self.observers.push(Arc::new(observer));
        self
    }

    /// Sets what happens when no route matches.
    ///
    /// Not a route: an unmatched path is outside the description entirely, and
    /// contributes no `paths` entry.
    #[must_use]
    pub fn not_found(mut self, policy: FallbackPolicy) -> Self {
        self.not_found = policy;
        self
    }

    /// Sets what happens when a path matches but the method does not.
    ///
    /// The `Allow` header is derived from the operations actually declared on
    /// that path, so it cannot disagree with the description.
    #[must_use]
    pub fn method_not_allowed(mut self, policy: FallbackPolicy) -> Self {
        self.method_not_allowed = policy;
        self
    }

    /// Sets the application-wide trailing-slash policy.
    ///
    /// Redirect mode only adds or removes the final slash to reach an exactly
    /// declared path. It never changes path casing or normalizes individual
    /// routes, and uses 308 so the request method and body are preserved.
    #[must_use]
    pub fn trailing_slashes(mut self, policy: TrailingSlashPolicy) -> Self {
        self.trailing_slashes = policy;
        self
    }

    /// Turns unconstrained-schema warnings into build errors.
    ///
    /// [`Unchecked`](crate::schema::unchecked::Unchecked) is honest but weak. A team that
    /// wants no weak schemas at all can say so here.
    #[must_use]
    pub fn deny_unchecked_schemas(mut self) -> Self {
        self.deny_unchecked_schemas = true;
        self
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
    /// Returns [`Error::Invalid`] if the router cannot be described at all.
    pub fn validate(&self) -> Result<Vec<Violation>>
    where
        C: 'static,
    {
        let described = self.describe()?;
        Ok(described.violations)
    }

    /// Produces the OpenAPI description, at the lowest version that expresses
    /// this API without loss.
    ///
    /// 3.1 for an API using no 3.2-only construct, and 3.2 for one that does —
    /// a `QUERY` operation, a streamed response, an `in: querystring`
    /// parameter. Lowest rather than highest, because a description a consumer
    /// can read is worth more than one that advertises a version number, and
    /// nothing is lost by saying 3.1 when 3.1 is enough.
    ///
    /// Note that this is *not* decided by the `openapi32` feature. Cargo
    /// unifies features across a dependency graph, so a crate elsewhere in the
    /// build enabling it would otherwise bump the version of a document whose
    /// own API never changed.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] when validation finds an error-level
    /// violation, so a misleading description is never emitted.
    pub fn openapi(&self) -> Result<Document>
    where
        C: 'static,
    {
        let described = self.describe()?;
        described.into_document()
    }

    /// Produces the description targeting a specific specification version.
    ///
    /// Targets, never downgrades: asking for a version that cannot express
    /// this API is an error listing what blocks it, not a document with the
    /// offending operations quietly missing. Reach for this when a consumer's
    /// toolchain pins a version, and let [`openapi`](Router::openapi) decide
    /// otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] on a validation error, or if the API uses a
    /// construct `version` cannot express — a Server-Sent Events response
    /// requested as 3.1, say.
    pub fn openapi_as(&self, version: SpecVersion) -> Result<Document>
    where
        C: 'static,
    {
        let described = self.describe()?;
        described.errors()?;
        described.document.emit(version).map_err(invalid)
    }

    /// Finalizes the router into something servable.
    ///
    /// This is where the structural checks run, so an API that cannot be
    /// described correctly fails at startup rather than at documentation time.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] with every violation found.
    pub fn build(self, context: C) -> Result<Service<C>>
    where
        C: Send + Sync + 'static,
    {
        let described = self.describe()?;
        let document = described.into_document()?;

        let mut matcher = matchit::Router::new();
        let mut paths: Vec<PathEntry<C>> = Vec::new();
        let mut index_of: HashMap<String, usize> = HashMap::new();

        for mounted in self.mounted {
            let key = mounted.path.as_str().to_owned();
            let index = if let Some(index) = index_of.get(&key) {
                *index
            } else {
                let index = paths.len();
                matcher.insert(key.clone(), index).map_err(|_| {
                    invalid(SpecError::DuplicatePathTemplate {
                        template: key.clone(),
                        existing: key.clone(),
                    })
                })?;
                paths.push(PathEntry {
                    template: key.clone(),
                    matched: crate::extract::connection::MatchedPath(dispatch::intern(&key)),
                    variables: mounted
                        .path
                        .variables()
                        .iter()
                        .map(|name| dispatch::intern(name))
                        .collect(),
                    allow: dispatch::allow_header(&[]),
                    operations: Vec::new(),
                });
                index_of.insert(key.clone(), index);
                index
            };

            let method = mounted.endpoint.method();
            let operation_id = document
                .paths
                .0
                .get(&key)
                .and_then(|item| item.operation(method))
                .and_then(|operation| operation.operation_id.clone())
                .unwrap_or_default();

            let mut interceptors = self.interceptors.clone();
            interceptors.extend(mounted.interceptors);

            #[cfg(feature = "unchecked")]
            let unchecked_layers = {
                let mut layers = self.unchecked.layers.clone();
                layers.extend(mounted.unchecked_layers);
                layers
            };

            paths[index].operations.push(dispatch::Served {
                method,
                operation_id,
                terminal: Arc::new(EndpointTerminal::new(mounted.endpoint)),
                interceptors,
                catch_panics: mounted.catch_panics || catches::<P>(),
                #[cfg(feature = "unchecked")]
                unchecked_layers,
            });
        }

        #[cfg(feature = "unchecked")]
        install_unchecked(
            &self.unchecked,
            &self.interceptors,
            catches::<P>(),
            &mut matcher,
            &mut paths,
            &mut index_of,
        )?;

        for entry in &mut paths {
            let methods: Vec<_> = entry
                .operations
                .iter()
                .map(|operation| operation.method)
                .collect();
            entry.allow = dispatch::allow_header(&methods);
        }

        // After the `Allow` loop, so the synthesized `OPTIONS` is in no `Allow`
        // header, and after `describe` has already run, so it is in no `paths`
        // key either. Both are properties of *when* this happens rather than of
        // a filter someone has to maintain.
        install_preflight(&mut paths, &self.method_not_allowed);

        let dispatch = Arc::new(Dispatch {
            matcher,
            paths,
            context,
            observers: self.observers,
            not_found: self.not_found,
            method_not_allowed: self.method_not_allowed,
            trailing_slashes: self.trailing_slashes,
        });

        Ok(Service::new(document, move |request| {
            let dispatch = Arc::clone(&dispatch);
            async move { dispatch.serve(request).await }
        }))
    }

    /// Assembles the description, and everything found on the way that a
    /// `Describe` implementation had no way to return.
    /// Registers every declared security scheme under `components`.
    ///
    /// A name the specification cannot hold as a component key is a violation
    /// rather than a failure: the rest of the description is still worth
    /// emitting, and `validate` is what decides whether it is usable.
    fn declare_security_schemes(&self, registry: &mut Registry, violations: &mut Vec<Violation>) {
        for (name, scheme) in &self.security_schemes {
            match kynos_openapi::ComponentName::new(*name) {
                Ok(name) => registry.declare_security_scheme(name, scheme.clone()),
                Err(_) => violations.push(error_at(
                    "#/components/securitySchemes",
                    SpecError::InvalidComponentName {
                        name: (*name).to_owned(),
                    },
                )),
            }
        }
    }

    /// Refuses an interceptor configured with a combination it cannot honour.
    ///
    /// Everything else an interceptor says is read from its types, so the
    /// compiler has already checked it. This is the one question about a
    /// *value*, and the only interceptor that has one is `Cors` — see
    /// [`cors_conflict`].
    ///
    /// Called from `describe` rather than `build` so that `validate`,
    /// `openapi`, `openapi_as` and `build` all report it, which is the same
    /// reason `Error::Contribution` is raised there.
    fn refuse_unhonourable_interceptors(&self) -> Result<()>
    where
        C: 'static,
    {
        for interceptor in self.interceptors.iter().chain(
            self.mounted
                .iter()
                .flat_map(|mounted| &mounted.interceptors),
        ) {
            if let Some(conflict) = cors_conflict(interceptor) {
                return Err(Error::Middleware(conflict));
            }
        }

        Ok(())
    }

    fn describe(&self) -> Result<Described>
    where
        C: 'static,
    {
        let mut registry = Registry::new();
        let mut violations = self.violations.clone();

        // Read before anything is described: a configuration that cannot be
        // honoured should not produce a document at all.
        self.refuse_unhonourable_interceptors()?;

        self.declare_security_schemes(&mut registry, &mut violations);

        let mut paths = Paths::new();
        for mounted in &self.mounted {
            let key = mounted.path.as_str().to_owned();
            let location = format!("#/paths/{}", pointer_token(&key));
            let method = mounted.endpoint.method();

            // The identifier is needed before the operation exists, because it
            // is half of the `Route` an interceptor is described against. A
            // throwaway registry keeps the probe from recording a conflict the
            // real pass is about to record again.
            let operation_id = {
                let mut probe = Registry::new();
                let mut cx = OperationCx::new(&mut probe);
                mounted.endpoint.describe(&mut cx);
                cx.finish().operation_id.unwrap_or_default()
            };
            let route = Route::new(&key, &operation_id, method);

            let mut cx = OperationCx::new(&mut registry);
            mounted.endpoint.describe(&mut cx);

            // The router's own interceptors are outermost, then whatever the
            // group or nested router contributed. The endpoint described itself
            // first, so its own responses win where the two overlap.
            for interceptor in self.interceptors.iter().chain(&mounted.interceptors) {
                interceptor.describe(route, &mut cx);
            }

            for tag in self.tags.iter().chain(&mounted.tags) {
                cx.add_tag(tag);
            }

            if mounted.catch_panics || catches::<P>() {
                let responses = dispatch::panic_responses(cx.registry());
                cx.add_responses(responses);
            }

            let operation = cx.finish();

            // A layer of undeclared effect covers this operation, so it stays
            // in `paths` and says it is no longer verified.
            #[cfg(feature = "unchecked")]
            let operation = {
                let mut operation = operation;
                if !self.unchecked.layers.is_empty() || !mounted.unchecked_layers.is_empty() {
                    // The only reachable failure is a marker already present in
                    // a shape Kynos never emits, which an operation Kynos just
                    // described cannot carry.
                    let _ = kynos_openapi::Opaque::new(kynos_openapi::OpaqueReason::UntypedLayer)
                        .apply_to(&mut operation);
                }
                operation
            };

            let item: &mut PathItem = paths.0.entry(key.clone()).or_default();
            if item.set_operation(method, operation).is_some() {
                violations.push(error_at(
                    format!("{location}/{}", method.as_wire_str().to_lowercase()),
                    SpecError::DuplicatePathTemplate {
                        template: format!("{method} {key}"),
                        existing: key,
                    },
                ));
            }
        }

        for check in &self.short_circuit_checks {
            if let Some(error) = check(&mut registry) {
                let violation = error_at("#", error);
                if !violations.contains(&violation) {
                    violations.push(violation);
                }
            }
        }

        if let Some(conflict) = registry.schema_conflicts().first() {
            return Err(Error::Schema(conflict.clone()));
        }
        if let Some(conflict) = registry.scheme_conflicts().first() {
            return Err(Error::Contribution(conflict.clone()));
        }

        let mut document = Document::new(
            highest_version(),
            self.info.clone().unwrap_or_else(placeholder_info),
        );
        document.servers.clone_from(&self.servers);
        document.paths = paths;
        document.tags = unique_tags(&self.tag_metadata);
        document.components = registry.into_components();

        // The version the description claims follows from what it uses, never
        // from a cargo feature: Cargo unifies features across a dependency
        // graph, so a flag some other crate turned on must not move it.
        let document = lowest_expressing(&document)?;

        // Before validation, because an opaque document that is not stamped is
        // an error the validator is entitled to raise.
        #[cfg(feature = "unchecked")]
        let document = {
            let mut document = document;
            self.unchecked.annotate(&mut document);
            document
        };

        let version = document.spec_version().unwrap_or_default();

        violations.extend(kynos_openapi::validate::Validator::new(version).validate(&document));

        if self.deny_unchecked_schemas {
            for violation in &mut violations {
                if violation.error == SpecError::UncheckedSchema {
                    violation.severity = Severity::Error;
                }
            }
        }

        Ok(Described {
            document,
            violations,
        })
    }
}

/// Adds the routes no path template expresses to the match table.
///
/// They reach the same table as every described route — they have to, or they
/// would not serve — and differ from one only in having no `paths` key to have
/// been derived from, and no variables to capture: an unchecked handler takes
/// the whole request and no extractor.
///
/// # Errors
///
/// Returns [`Error::Invalid`] when a pattern collides with one already in the
/// table under a different key.
#[cfg(feature = "unchecked")]
fn install_unchecked<C>(
    unchecked: &crate::unchecked::Unchecked<C>,
    interceptors: &[Arc<dyn ErasedInterceptor<C>>],
    catch_panics: bool,
    matcher: &mut matchit::Router<usize>,
    paths: &mut Vec<PathEntry<C>>,
    index_of: &mut HashMap<String, usize>,
) -> Result<()> {
    for route in &unchecked.routes {
        let key = route.pattern.clone();
        let index = if let Some(index) = index_of.get(&key) {
            *index
        } else {
            let index = paths.len();
            matcher.insert(key.clone(), index).map_err(|_| {
                invalid(SpecError::DuplicatePathTemplate {
                    template: key.clone(),
                    existing: key.clone(),
                })
            })?;
            paths.push(PathEntry {
                template: key.clone(),
                matched: crate::extract::connection::MatchedPath(dispatch::intern(&key)),
                // Read from the matching pattern rather than from a
                // `PathTemplate`, which a catch-all is not one of. Leaving this
                // empty made `Dispatch`'s capture branch unreachable, so an
                // unchecked handler had to re-derive what the matcher had
                // already taken apart — including the decoding and the `..`
                // rejection `extract/params/path.rs` keeps private.
                variables: matcher_variables(&key),
                allow: dispatch::allow_header(&[]),
                operations: Vec::new(),
            });
            index_of.insert(key, index);
            index
        };

        let mut unchecked_layers = unchecked.layers.clone();
        unchecked_layers.extend(route.layers.iter().cloned());

        for method in &route.methods {
            paths[index].operations.push(dispatch::Served {
                // Distinct per route and per method, because `Next::route` hands
                // this to every interceptor: an empty string collided every
                // unchecked route into one rate-limit bucket and one metric
                // label. `unchecked:` marks it as synthesized rather than
                // something a document declares, since no document declares it.
                operation_id: format!("unchecked:{} {}", method.as_wire_str(), route.pattern),
                method: *method,
                terminal: Arc::clone(&route.terminal),
                interceptors: interceptors.to_vec(),
                catch_panics,
                unchecked_layers: unchecked_layers.clone(),
            });
        }
    }

    Ok(())
}

/// The variable names a matching pattern captures.
///
/// The router's own syntax rather than a path template: `{name}` captures
/// `name` and `{*name}` captures `name` too, since matchit reports a catch-all
/// under the bare name. A segment that is not a variable captures nothing.
#[cfg(feature = "unchecked")]
fn matcher_variables(pattern: &str) -> Vec<&'static str> {
    pattern
        .split('/')
        .filter_map(|segment| {
            let name = segment.strip_prefix('{')?.strip_suffix('}')?;
            let name = name.strip_prefix('*').unwrap_or(name);
            (!name.is_empty()).then(|| dispatch::intern(name))
        })
        .collect()
}

/// A described router: the document it produces, and everything wrong with it.
struct Described {
    document: Document,
    violations: Vec<Violation>,
}

impl Described {
    /// Fails when any violation is error-level, so a misleading description is
    /// never emitted.
    fn errors(&self) -> Result<()> {
        let errors: Vec<Violation> = self
            .violations
            .iter()
            .filter(|violation| violation.severity == Severity::Error)
            .cloned()
            .collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::Invalid { violations: errors })
        }
    }

    fn into_document(self) -> Result<Document> {
        self.errors()?;
        Ok(self.document)
    }
}

/// Whether `P` selected recovery.
///
/// [`PanicPolicy`] is a marker with no members to read, so the policy is
/// resolved by identity — which is exactly as static as the type it comes from.
fn catches<P: PanicPolicy>() -> bool {
    std::any::TypeId::of::<P>() == std::any::TypeId::of::<Catch>()
}

/// The highest version this build can express, which is what a description is
/// assembled at before it is emitted downwards.
fn highest_version() -> SpecVersion {
    #[cfg(feature = "openapi32")]
    {
        SpecVersion::V3_2
    }
    #[cfg(not(feature = "openapi32"))]
    {
        SpecVersion::V3_1
    }
}

/// The document at the lowest version expressing it without loss.
///
/// [`Document::emit`] already knows which constructs block a downgrade, so this
/// asks it rather than repeating the analysis.
fn lowest_expressing(document: &Document) -> Result<Document> {
    match document.emit(SpecVersion::V3_1) {
        Ok(emitted) => Ok(emitted),
        #[cfg(feature = "openapi32")]
        Err(_) => document.emit(SpecVersion::V3_2).map_err(invalid),
        #[cfg(not(feature = "openapi32"))]
        Err(blocked) => Err(invalid(blocked)),
    }
}

/// The `info` block a router that declared none still has to emit.
///
/// OpenAPI requires a title and a version, so there is no honest way to omit
/// them; a visible placeholder is better than a plausible invention.
fn placeholder_info() -> Info {
    Info::new("API", "0.0.0")
}

/// Tag metadata with the first claim on each name kept.
fn unique_tags(declared: &[kynos_openapi::Tag]) -> Vec<kynos_openapi::Tag> {
    let mut tags: Vec<kynos_openapi::Tag> = Vec::new();
    for tag in declared {
        if !tags.iter().any(|existing| existing.name == tag.name) {
            tags.push(tag.clone());
        }
    }
    tags
}

/// Why Kynos will not route a path its model can nonetheless hold.
///
/// Registers a preflight answer on every path a `Cors` covers.
///
/// One `Served` per path rather than a branch in `Dispatch::serve`: a preflight
/// then flows through the machinery that already exists — the matcher finds the
/// path, `position` finds the method — and `Dispatch` needs to hold no CORS
/// configuration of its own.
///
/// Skipped where the path already declares `OPTIONS`. A hand-written operation
/// wins, and it wins by construction rather than by a race in `position`'s
/// linear scan.
///
/// The interceptor list on the synthesized entry is deliberately empty. A
/// browser sends a preflight with no credentials and no `Authorization`, so an
/// auth interceptor short-circuiting it would break CORS for every operation on
/// the path — and `docs/middleware.md` says an interceptor covers the
/// *operations* in its subtree, which a preflight is not. Observers still see
/// it, because they sit outside the chain.
/// One `Cors` mounted over a path, and the methods on that path it covers.
///
/// Borrowed rather than cloned: the identity of the `Arc` is what tells two
/// mounted configurations apart, and a clone of the configuration cannot be
/// compared once one of them can hold a predicate.
type CoveringCors<'a, C> = (
    &'a Arc<dyn ErasedInterceptor<C>>,
    Vec<kynos_openapi::Method>,
);

fn install_preflight<C: Send + Sync + 'static>(
    paths: &mut [PathEntry<C>],
    method_not_allowed: &FallbackPolicy,
) {
    for entry in paths {
        if entry
            .operations
            .iter()
            .any(|operation| operation.method == kynos_openapi::Method::Options)
        {
            continue;
        }

        // Every configuration covering this path, and the methods each one
        // covers. An interceptor mounted on a group owning `GET /x` while the
        // router owns `POST /x` advertises `GET` only, which is what keeps
        // preflight and the description agreeing about what exists.
        //
        // More than one is reachable: a group's stack is checked against the
        // router's and never against a sibling's, so two groups may cover one
        // path with a `Cors` each. Grouped by the interceptor's identity, since
        // that is what "the same `Cors`" means once a configuration can hold a
        // predicate no comparison could see through.
        let mut scopes: Vec<CoveringCors<'_, C>> = Vec::new();

        for operation in &entry.operations {
            let Some(found) = operation
                .interceptors
                .iter()
                .find(|interceptor| cors_config(interceptor).is_some())
            else {
                continue;
            };

            if let Some((_, covered)) = scopes
                .iter_mut()
                .find(|(mounted, _)| Arc::ptr_eq(mounted, found))
            {
                covered.push(operation.method);
            } else {
                scopes.push((found, vec![operation.method]));
            }
        }

        if scopes.is_empty() {
            continue;
        }

        let scopes = scopes
            .into_iter()
            .map(|(interceptor, covered)| {
                let config = cors_config(interceptor).expect("a recognised CORS interceptor");
                crate::middleware::cors::preflight::Scope::new(config.clone(), covered)
            })
            .collect();

        let preflight = crate::middleware::cors::preflight::Preflight::new(
            scopes,
            entry.allow.clone(),
            method_not_allowed.clone(),
        );

        entry.operations.push(dispatch::Served {
            method: kynos_openapi::Method::Options,
            operation_id: String::new(),
            terminal: Arc::new(dispatch::PreflightTerminal::new(preflight)),
            interceptors: Vec::new(),
            catch_panics: false,
            #[cfg(feature = "unchecked")]
            unchecked_layers: Vec::new(),
        });
    }
}

/// The CORS configuration an interceptor carries, if it is one.
fn cors_config<C: 'static>(
    interceptor: &Arc<dyn ErasedInterceptor<C>>,
) -> Option<&crate::middleware::cors::CorsConfig> {
    use crate::middleware::cors::{Cors, Documented, Undocumented};

    let value = interceptor.as_any();

    value
        .downcast_ref::<Cors<Undocumented>>()
        .map(Cors::config)
        .or_else(|| value.downcast_ref::<Cors<Documented>>().map(Cors::config))
}

/// The configuration conflict an interceptor carries, if it is one the router
/// recognises and it has one.
///
/// The only place Kynos reads an interceptor as a *value* rather than through
/// its types, and it is deliberately not a capability: the match below is a
/// closed list of two, `Cors`'s state parameter is sealed so there cannot be a
/// third, and a third-party interceptor is never asked. Nothing read here
/// reaches the description.
fn cors_conflict<C: 'static>(
    interceptor: &Arc<dyn ErasedInterceptor<C>>,
) -> Option<crate::middleware::MiddlewareError> {
    cors_config(interceptor).and_then(crate::middleware::cors::CorsConfig::conflict)
}

/// The routing contract is narrower than the document model on purpose: a
/// catch-all matches a set of paths no single template describes, and a segment
/// carrying two variables is a shape the matcher cannot take apart. Both checks
/// belong here rather than in `PathTemplate`, which has to round-trip a
/// description it did not produce.
fn unroutable(path: &kynos_openapi::PathTemplate) -> Option<SpecError> {
    let catch_all = path.variables().iter().any(|name| name.starts_with('*'));
    let crowded = path
        .normalized()
        .split('/')
        .any(|segment| segment.matches("{}").count() > 1);

    (catch_all || crowded).then(|| SpecError::OpaqueRoute {
        pattern: path.as_str().to_owned(),
    })
}

/// One error-level violation.
fn error_at(location: impl Into<String>, error: SpecError) -> Violation {
    Violation {
        location: location.into(),
        severity: Severity::Error,
        error,
    }
}

/// One error-level violation, as the framework error carrying it.
fn invalid(error: SpecError) -> Error {
    Error::Invalid {
        violations: vec![error_at("#", error)],
    }
}

/// Escapes one `paths` key for use as a JSON Pointer token, per RFC 6901.
///
/// Every key contains a `/`, so a location embedding one unescaped reads as
/// several tokens and resolves against nothing.
fn pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
