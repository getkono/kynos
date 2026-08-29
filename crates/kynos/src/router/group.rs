//! A set of operations sharing a prefix, a tag, and interceptors.

use std::{marker::PhantomData, sync::Arc};

use kynos_openapi::{PathTemplate, Severity, SpecError, Violation};

use crate::{
    middleware::{
        Interceptor,
        catch_panic::{Catch, PanicPolicy, Propagate},
        erased::ErasedInterceptor,
        stack::{CompatibleStack, CompatibleWith, Cons, Flatten},
    },
    response::short_circuit_mismatch,
    router::{
        ShortCircuitCheck,
        endpoint::{DynEndpoint, set::IntoEndpoints},
        operation::Tag,
    },
};

/// A set of operations sharing a path prefix, a tag, and interceptors.
///
/// This is the recommended unit of API structure: one group per resource. The
/// prefix becomes part of each path, the tag is applied to each operation, and
/// each interceptor's contribution is merged into each operation's description
/// — so attaching authentication to a group documents it on every operation
/// underneath, correctly, without anyone maintaining that by hand.
pub struct Group<C, P = Propagate, I = (), S = ()> {
    prefix: String,
    endpoints: Vec<Arc<dyn DynEndpoint<C>>>,
    interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,
    short_circuit_checks: Vec<ShortCircuitCheck>,
    tags: Vec<&'static str>,
    tag_metadata: Vec<kynos_openapi::Tag>,
    /// Problems found while the group was assembled, which the fluent methods
    /// cannot return. A router takes them over when it mounts the group.
    violations: Vec<Violation>,
    /// Layers of undeclared effect covering this group's operations, outermost
    /// first. `pub(crate)` because `unchecked` mounts them.
    #[cfg(feature = "unchecked")]
    pub(crate) unchecked_layers: Vec<Arc<dyn crate::unchecked::ErasedLayer>>,

    // See `Router`: the parameters name a shape, not this value's auto traits.
    // `I` is the interceptors mounted here as a type-level list, and `S` is
    // what the endpoints mounted here brought with them — kept apart, because
    // `I` covers every operation in the group and so must be checked against
    // an incoming stack, while `S` covers subtrees and must not.
    // The lint is measuring the four parameters the type genuinely
    // has; factoring them into an alias would hide the shape rather
    // than simplify it.
    #[allow(clippy::type_complexity)]
    _private: PhantomData<fn() -> (C, P, I, S)>,
}

impl<C, P, I, S> std::fmt::Debug for Group<C, P, I, S> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Group")
            .field("prefix", &self.prefix)
            .field("operations", &self.endpoints.len())
            .field("interceptors", &self.interceptors.len())
            .finish_non_exhaustive()
    }
}

/// Everything a group holds, once a router has taken it over.
pub(crate) struct GroupParts<C> {
    pub(crate) prefix: String,
    pub(crate) endpoints: Vec<Arc<dyn DynEndpoint<C>>>,
    pub(crate) interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,
    pub(crate) short_circuit_checks: Vec<ShortCircuitCheck>,
    pub(crate) tags: Vec<&'static str>,
    pub(crate) tag_metadata: Vec<kynos_openapi::Tag>,
    pub(crate) violations: Vec<Violation>,
    #[cfg(feature = "unchecked")]
    pub(crate) unchecked_layers: Vec<Arc<dyn crate::unchecked::ErasedLayer>>,
}

impl<C> Group<C, Propagate, ()> {
    /// Creates a group mounted at `prefix`.
    #[must_use]
    pub fn new(prefix: &str) -> Self {
        // `PathTemplate` is the only parser for a path in the workspace, so the
        // prefix is checked by the same rules the paths beneath it are. A
        // malformed one is recorded rather than returned, because a builder
        // method that returned a `Result` would make every group two lines.
        let (prefix, violations) = match PathTemplate::parse(prefix) {
            Ok(template) => (template.as_str().to_owned(), Vec::new()),
            Err(reason) => (
                String::new(),
                vec![Violation {
                    location: "#/paths".to_owned(),
                    severity: Severity::Error,
                    error: SpecError::InvalidPathTemplate {
                        template: prefix.to_owned(),
                        reason,
                    },
                }],
            ),
        };

        Self {
            prefix,
            endpoints: Vec::new(),
            interceptors: Vec::new(),
            short_circuit_checks: Vec::new(),
            tags: Vec::new(),
            tag_metadata: Vec::new(),
            violations,
            #[cfg(feature = "unchecked")]
            unchecked_layers: Vec::new(),
            _private: PhantomData,
        }
    }
}

impl<C, P, I, S> Group<C, P, I, S> {
    /// Carries every field across a change of type parameter.
    fn retype<Q, J, T>(self) -> Group<C, Q, J, T> {
        Group {
            prefix: self.prefix,
            endpoints: self.endpoints,
            interceptors: self.interceptors,
            short_circuit_checks: self.short_circuit_checks,
            tags: self.tags,
            tag_metadata: self.tag_metadata,
            violations: self.violations,
            #[cfg(feature = "unchecked")]
            unchecked_layers: self.unchecked_layers,
            _private: PhantomData,
        }
    }

    /// Hands everything this group holds to the router mounting it.
    pub(crate) fn into_parts(self) -> GroupParts<C> {
        GroupParts {
            prefix: self.prefix,
            endpoints: self.endpoints,
            interceptors: self.interceptors,
            short_circuit_checks: self.short_circuit_checks,
            tags: self.tags,
            tag_metadata: self.tag_metadata,
            violations: self.violations,
            #[cfg(feature = "unchecked")]
            unchecked_layers: self.unchecked_layers,
        }
    }
}

impl<C, P: PanicPolicy, I, S> Group<C, P, I, S> {
    /// Converts panics from covered operations into documented 500 responses.
    ///
    /// The policy is carried in the group's type and resolved while its
    /// endpoints are mounted. No recovery branch is installed when this method
    /// is not called.
    ///
    /// Only the policy changes; `I` is carried across for the reason
    /// [`Router::catch_panics`](crate::Router::catch_panics) gives.
    ///
    /// # Compile-time requirement
    ///
    /// The final binary must use `panic = "unwind"`. Selecting this policy in
    /// a `panic = "abort"` build is a compile-time error.
    ///
    /// ```no_run
    /// let users = kynos::router::group::Group::<()>::new("/users").catch_panics();
    /// # let _ = users;
    /// ```
    #[must_use]
    pub fn catch_panics(self) -> Group<C, Catch, I, S> {
        const {
            assert!(
                cfg!(panic = "unwind"),
                "Kynos panic recovery requires `panic = \"unwind\"`; remove `catch_panics` or enable unwinding"
            );
        }
        self.retype()
    }

    /// Tags every operation in this group.
    #[must_use]
    pub fn tag<T: Tag>(mut self) -> Self {
        self.tags.push(T::NAME);
        self.tag_metadata.push(T::metadata());
        self
    }

    /// Applies an interceptor to every operation in this group.
    ///
    /// The first call is the outermost of the group's own, and every one of
    /// them sits inside whatever the enclosing router applied; see
    /// [the module's ordering rule](crate::middleware#the-order-a-chain-runs-in).
    ///
    /// Checked against the group's own interceptors *and* against the stacks
    /// the endpoints already mounted here brought with them, since this covers
    /// both.
    #[must_use]
    pub fn intercept<N: Interceptor<C>>(self, interceptor: N) -> Group<C, P, Cons<N, I>, S>
    where
        C: Sync + 'static,
        I: CompatibleWith<N, C>,
        S: CompatibleWith<N, C>,
    {
        let () = <I as CompatibleWith<N, C>>::CHECK;
        // The endpoints mounted before this call are covered by it too, so
        // mounting first and intercepting second has to be checked exactly as
        // the other order is.
        let () = <S as CompatibleWith<N, C>>::CHECK;

        let mut group: Group<C, P, Cons<N, I>, S> = self.retype();
        group.interceptors.push(Arc::new(interceptor));
        group
            .short_circuit_checks
            .push(short_circuit_mismatch::<N::Short>);
        group
    }

    /// Mounts operations into this group.
    ///
    /// What the endpoints carry is checked against the group's own
    /// interceptors and then remembered, so a later [`intercept`] sees it. It
    /// is *not* checked against what an earlier `mount` left behind: two
    /// operations never collide with each other, since no request reaches
    /// both.
    ///
    /// Mounting operations that carry no interceptor leaves this type
    /// unchanged, because [`Flatten`] erases an empty stack.
    ///
    /// [`intercept`]: Group::intercept
    #[must_use]
    pub fn mount<E: IntoEndpoints<C>>(
        mut self,
        endpoints: E,
    ) -> Group<C, P, I, <E::Stacks as Flatten<S>>::Out>
    where
        E::Stacks: CompatibleStack<I, C> + Flatten<S>,
    {
        let () = <E::Stacks as CompatibleStack<I, C>>::CHECK;

        let mut sink = crate::router::endpoint::set::Endpoints::new();
        endpoints.into_endpoints(&mut sink);
        self.endpoints.extend(sink.into_inner());
        self.retype()
    }
}
