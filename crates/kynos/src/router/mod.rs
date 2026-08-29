//! Routing, grouping, and the path from code to description.
//!
//! [`Router`] is the root and lives here. [`endpoint`] holds one declared
//! operation and the builder a route attribute expands into, [`group`] a set of
//! operations sharing a prefix, [`operation`] the description being assembled,
//! [`policy`] the application-wide fallbacks, and [`service`] the built result.

mod describe;
mod install;

use install::{catches, error_at, pointer_token, unroutable};

#[cfg(feature = "assets")]
pub mod assets;
#[cfg(feature = "docs")]
pub mod docs;
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
        stack::{CompatibleStack, CompatibleWith, Cons, Flatten},
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
    /// Which half of a reference this is, when it is one.
    ///
    /// Here rather than in a list of its own because `absorb_router` has
    /// already applied every enclosing prefix to `path` by the time this is
    /// read, and because an entry dropped for a violation takes its half of the
    /// mount with it rather than leaving a page pointed at nothing.
    #[cfg(feature = "docs")]
    pub(crate) docs: Option<docs::Role>,
}

/// The root of an API.
///
/// `C` is the application context type: the application's own struct, which
/// every handler resolves its state from. It is a type rather than a
/// container — nothing is registered into it and nothing is looked up — so a
/// handler asking for something the context does not provide is a compile
/// error rather than a runtime panic.
pub struct Router<C, P = Propagate, I = (), S = ()> {
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
    pub(crate) trusted_proxies: crate::http::forwarded::TrustedProxies,
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
    //
    // `S` is what the scopes mounted here brought with them: a group's own
    // interceptors, a nested router's, an endpoint's. Two parameters rather
    // than one, because the two are checked in opposite directions. `I` covers
    // every operation, so an incoming sub-stack must clear it. `S` covers
    // subtrees, so an incoming sub-stack must *not* be compared against it --
    // two sibling groups may hold one interceptor, since no request reaches
    // both. Only `intercept`, which covers everything, reads `S`.
    // The lint is measuring the four parameters the type genuinely
    // has; factoring them into an alias would hide the shape rather
    // than simplify it.
    #[allow(clippy::type_complexity)]
    _private: PhantomData<fn() -> (C, P, I, S)>,
}

impl<C, P, I, S> std::fmt::Debug for Router<C, P, I, S> {
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

impl<C, P, I, S> Router<C, P, I, S> {
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
            trusted_proxies: crate::http::forwarded::TrustedProxies::none(),
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
    fn retype<Q, J, T>(self) -> Router<C, Q, J, T> {
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
            trusted_proxies: self.trusted_proxies,
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
                #[cfg(feature = "docs")]
                docs: None,
            });
        }
    }

    /// Takes in everything another router holds, under `prefix`.
    fn absorb_router<Q, J, T>(
        &mut self,
        other: Router<C, Q, J, T>,
        prefix: &str,
        catch_panics: bool,
    ) where
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

impl<C, P: PanicPolicy, I, S> Router<C, P, I, S> {
    /// Converts panics from covered operations into documented 500 responses.
    ///
    /// The policy is carried in the router's type and resolved when the service
    /// is built. No recovery branch is installed when this method is not
    /// called.
    ///
    /// Only the policy changes. `I` is carried across, because the
    /// interceptors mounted before this call cover the operations mounted
    /// after it just as they did before — so dropping the list here would let
    /// a later `intercept` be checked against an empty one.
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
    pub fn catch_panics(self) -> Router<C, Catch, I> {
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
    ///
    /// What the endpoints carry is checked against this router's own
    /// interceptors and then remembered, so a later [`intercept`] sees it.
    /// Mounting operations that carry none leaves this type unchanged, because
    /// [`Flatten`] erases an empty stack — which is what keeps re-assignment
    /// and a conditional mount compiling.
    ///
    /// [`intercept`]: Router::intercept
    #[must_use]
    pub fn mount<E: IntoEndpoints<C>>(
        mut self,
        endpoints: E,
    ) -> Router<C, P, I, <E::Stacks as Flatten<S>>::Out>
    where
        C: 'static,
        E::Stacks: CompatibleStack<I, C> + Flatten<S>,
    {
        let () = <E::Stacks as CompatibleStack<I, C>>::CHECK;

        let mut sink = endpoint::set::Endpoints::new();
        endpoints.into_endpoints(&mut sink);
        self.absorb(sink.into_inner(), "", &[], &[], false);
        self.retype()
    }

    /// Mounts an API reference: the page a human opens, and the description it
    /// fetches.
    ///
    /// Both are ordinary described operations, so the document gains two
    /// `paths` keys and says so. See [`router::docs`](docs) for what that costs
    /// and why it is not hidden.
    ///
    /// The description is serialized while this router is built, because it has
    /// to describe these two routes -- and the page is pointed at the path the
    /// description actually got, so nesting moves both halves together.
    ///
    /// Whether a deployment exposes its reference stays the deployment's
    /// decision. This returns `Self` rather than changing the router's type, so
    /// both arms of a conditional agree:
    ///
    /// ```no_run
    /// use kynos::{Router, router::docs::Docs};
    ///
    /// let router = Router::<()>::new();
    /// let router = if std::env::var("DOCS").is_ok() {
    ///     router.docs(Docs::scalar())
    /// } else {
    ///     router
    /// };
    /// # let _ = router;
    /// ```
    #[cfg(feature = "docs")]
    #[must_use]
    pub fn docs(mut self, mut docs: docs::Docs) -> Self
    where
        C: Send + Sync + 'static,
    {
        // A path literal `Docs` could not parse is recorded there and drained
        // here, so it reaches `validate` alongside every other malformed path
        // rather than panicking at the mount site.
        self.violations.extend(docs.take_violations());

        // Through `absorb` one half at a time, so a docs path meets the same two
        // rules every other path does and the role stays unambiguous when a
        // violation drops the entry.
        for (endpoint, role) in docs.into_halves() {
            let first = self.mounted.len();
            self.absorb(vec![endpoint], "", &[], &[], false);
            for mounted in &mut self.mounted[first..] {
                mounted.docs = Some(role.clone());
            }
        }

        self
    }

    /// Mounts a group.
    ///
    /// Both of the group's stacks are checked against this router's own: its
    /// interceptors, and whatever the endpoints mounted inside it carried. The
    /// second is what makes a group's endpoint-scoped interceptor visible here
    /// at all. Both are then remembered, so a later [`intercept`] — which
    /// covers this group too — is checked against them.
    ///
    /// Neither is checked against what an *earlier* `group` left behind. Two
    /// groups cover different operations, so two of them holding one
    /// interceptor is not a collision.
    ///
    /// [`intercept`]: Router::intercept
    #[must_use]
    pub fn group<GP: PanicPolicy, GI, GS>(
        mut self,
        group: Group<C, GP, GI, GS>,
    ) -> Router<C, P, I, <GI as Flatten<<GS as Flatten<S>>::Out>>::Out>
    where
        C: 'static,
        GS: CompatibleStack<I, C> + Flatten<S>,
        GI: CompatibleStack<I, C> + Flatten<<GS as Flatten<S>>::Out>,
    {
        let () = <GI as CompatibleStack<I, C>>::CHECK;
        let () = <GS as CompatibleStack<I, C>>::CHECK;

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

        self.retype()
    }

    /// Mounts another router beneath a path prefix.
    ///
    /// Both of the nested router's stacks are checked and remembered, for the
    /// reason [`group`](Router::group) gives. `NS` is the load-bearing half:
    /// without it, a group mounted inside the nested router appears in no type
    /// this one can see, and its interceptors are compared against nothing in
    /// either order.
    #[must_use]
    pub fn nest<NP: PanicPolicy, NI, NS>(
        mut self,
        prefix: &str,
        router: Router<C, NP, NI, NS>,
    ) -> Router<C, P, I, <NI as Flatten<<NS as Flatten<S>>::Out>>::Out>
    where
        C: 'static,
        NS: CompatibleStack<I, C> + Flatten<S>,
        NI: CompatibleStack<I, C> + Flatten<<NS as Flatten<S>>::Out>,
    {
        let () = <NI as CompatibleStack<I, C>>::CHECK;
        let () = <NS as CompatibleStack<I, C>>::CHECK;

        self.absorb_router(router, prefix, catches::<NP>());
        self.retype()
    }

    /// Merges another router at the same level.
    ///
    /// The same two checks [`nest`](Router::nest) makes, at the same level
    /// rather than beneath a prefix.
    #[must_use]
    pub fn merge<OP: PanicPolicy, OI, OS>(
        mut self,
        router: Router<C, OP, OI, OS>,
    ) -> Router<C, P, I, <OI as Flatten<<OS as Flatten<S>>::Out>>::Out>
    where
        C: 'static,
        OS: CompatibleStack<I, C> + Flatten<S>,
        OI: CompatibleStack<I, C> + Flatten<<OS as Flatten<S>>::Out>,
    {
        let () = <OI as CompatibleStack<I, C>>::CHECK;
        let () = <OS as CompatibleStack<I, C>>::CHECK;

        self.absorb_router(router, "", catches::<OP>());
        self.retype()
    }

    /// Declares a security scheme the API can use.
    // `Scheme` rather than `S`, which the router's own sub-stack parameter now
    // takes. Turbofish is positional, so no caller spells this name.
    #[must_use]
    pub fn security_scheme<Scheme: SecurityScheme>(mut self) -> Self {
        self.security_schemes
            .push((Scheme::NAME, Scheme::describe()));
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
    pub fn intercept<N: Interceptor<C>>(self, interceptor: N) -> Router<C, P, Cons<N, I>, S>
    where
        C: Sync + 'static,
        I: CompatibleWith<N, C>,
        S: CompatibleWith<N, C>,
    {
        // Forcing the const is what puts the error on this call rather than in
        // `middleware::stack`. Two interceptors adding one header, or
        // answering with one status, stop here.
        let () = <I as CompatibleWith<N, C>>::CHECK;
        // And against the scopes already mounted, which this covers as surely
        // as it covers what was intercepted here. Without it the check was an
        // ordering accident: `intercept` before `group` was refused and
        // `group` before `intercept` was not, though the chain that runs is
        // the same either way.
        let () = <S as CompatibleWith<N, C>>::CHECK;

        let mut router: Router<C, P, Cons<N, I>, S> = self.retype();
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

    /// Names the proxies whose forwarding fields may be believed.
    ///
    /// One application-level policy, or none — the rule
    /// [`trailing_slashes`](Self::trailing_slashes) follows, and for a sharper
    /// reason: two limiters disagreeing about which hop to trust would be two
    /// answers to one security question.
    ///
    /// Unset, nothing is believed. RFC 7239 section 8.1 says the field "cannot
    /// be relied upon to be correct, as it may be modified, whether mistakenly
    /// or for malicious reasons, by every node on the way to the server,
    /// including the client making the request" — so a default that read it
    /// would let any client choose the address its rate limit counts against.
    ///
    /// ```no_run
    /// use kynos::{Router, http::forwarded::TrustedProxies};
    ///
    /// let router = Router::<()>::new().trusted_proxies(TrustedProxies::hops(1));
    /// # let _ = router;
    /// ```
    #[must_use]
    pub fn trusted_proxies(mut self, trusted: crate::http::forwarded::TrustedProxies) -> Self {
        self.trusted_proxies = trusted;
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
}
