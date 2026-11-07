//! A set of operations sharing a prefix, tags, and interceptors.

use std::{marker::PhantomData, sync::Arc};

use kynos_openapi::Violation;

use crate::{
    middleware::{
        Interceptor,
        catch_panic::{Catch, PanicPolicy, Propagate},
        erased::ErasedInterceptor,
        stack::{CompatibleStack, CompatibleWith, Cons, Flatten, InterceptorList},
    },
    response::ShortCircuit,
    router::error::RouterError,
    router::{
        ShortCircuitCheck,
        endpoint::{DynEndpoint, set::IntoEndpoints},
        install,
        operation::{DeclaredTag, Tag},
    },
    schema::registry::Registry,
};

/// A set of operations sharing a path prefix, tags, and interceptors.
///
/// This is the recommended unit of API structure: one group per resource. The
/// prefix becomes part of each path, every tag the group names is applied to
/// each operation, and
/// each interceptor's contribution is merged into each operation's description
/// — so attaching authentication to a group documents it on every operation
/// underneath.
pub struct Group<C, P = Propagate, I = (), S = ()> {
    // `pub(super)`: the router mounting a group destructures it.
    pub(super) prefix: String,
    pub(super) endpoints: Vec<Arc<dyn DynEndpoint<C>>>,
    pub(super) interceptors: Vec<Arc<dyn ErasedInterceptor<C>>>,
    pub(super) short_circuit_checks: Vec<ShortCircuitCheck>,
    /// Where in `interceptors` each covered operation's guard runs, once
    /// [`guard_first`](Group::guard_first) asked: the number of them outside
    /// it.
    pub(super) guard_first: Option<usize>,
    pub(super) tags: Vec<DeclaredTag>,
    /// Kept beside `tags`, as on `Router`, so a tag covering nothing is still
    /// documented.
    pub(super) tag_metadata: Vec<kynos_openapi::Tag>,
    /// Problems found while the group was assembled, which the fluent methods
    /// cannot return.
    pub(super) violations: Vec<Violation<RouterError>>,
    /// Layers of undeclared effect covering this group's operations, outermost
    /// first. `pub(crate)` because `unchecked` mounts them.
    #[cfg(feature = "unchecked")]
    pub(crate) unchecked_layers: Vec<Arc<dyn crate::unchecked::ErasedLayer>>,

    // As on `Router`: `I` is this group's interceptor list, `S` what its
    // endpoints brought.
    #[allow(clippy::type_complexity)]
    pub(super) _private: PhantomData<fn() -> (C, P, I, S)>,
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

impl<C> Group<C, Propagate, ()> {
    /// Creates a group mounted at `prefix`.
    #[must_use]
    pub fn new(prefix: &str) -> Self {
        // A malformed prefix is recorded as a violation rather than returned.
        let mut violations = Vec::new();
        let prefix = install::parse_or_record(prefix, &mut violations)
            .map(|template| template.as_str().to_owned())
            .unwrap_or_default();

        Self {
            prefix,
            endpoints: Vec::new(),
            interceptors: Vec::new(),
            short_circuit_checks: Vec::new(),
            guard_first: None,
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
            guard_first: self.guard_first,
            tags: self.tags,
            tag_metadata: self.tag_metadata,
            violations: self.violations,
            #[cfg(feature = "unchecked")]
            unchecked_layers: self.unchecked_layers,
            _private: PhantomData,
        }
    }
}

impl<C, P: PanicPolicy, I, S> Group<C, P, I, S> {
    /// Converts panics from covered operations into documented 500 responses.
    ///
    /// The policy is carried in the group's type and resolved when the group is
    /// added to a router by [`Router::group`](crate::router::Router::group).
    /// No recovery branch is installed when this method
    /// is not called.
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
        const { install::require_unwind() }
        self.retype()
    }

    /// Tags every operation in this group.
    #[must_use]
    pub fn tag<T: Tag>(mut self) -> Self {
        self.tags.push(DeclaredTag::of::<T>());
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
        // Endpoints mounted earlier are covered too.
        let () = <S as CompatibleWith<N, C>>::CHECK;

        let mut group: Group<C, P, Cons<N, I>, S> = self.retype();
        group.interceptors.push(Arc::new(interceptor));
        group
            .short_circuit_checks
            .push(short_circuit_mismatch::<N::Short>);
        group
    }

    /// Applies each interceptor in `chain` to every operation in this group,
    /// first element outermost.
    ///
    /// Exactly [`intercept`](Group::intercept) once per element, in order, and
    /// checked as those calls are: each element against the group's stack, the
    /// endpoints already mounted, and the elements before it. What it adds is
    /// that a chain is one value, so a function can hand it out, and that the
    /// group it returns is named with [`stack!`](crate::stack!) in the same
    /// order the tuple is written.
    ///
    /// ```no_run
    /// use kynos::{
    ///     middleware::{catch_panic::Propagate, limits::body_size::BodySize, request_id::RequestId},
    ///     router::group::Group,
    /// };
    ///
    /// fn uploads() -> Group<(), Propagate, kynos::stack![RequestId, BodySize]> {
    ///     Group::new("/uploads").intercept_all((RequestId::new(), BodySize::new(1 << 20)))
    /// }
    /// # let _ = uploads();
    /// ```
    ///
    /// Two elements that collide fail to compile, as two `intercept` calls do:
    ///
    /// ```compile_fail,E0080
    /// use kynos::{middleware::request_id::RequestId, router::group::Group};
    ///
    /// let _ = Group::<()>::new("/a").intercept_all((RequestId::new(), RequestId::new()));
    /// ```
    #[must_use]
    pub fn intercept_all<L: InterceptorList<C, I, S>>(self, chain: L) -> Group<C, P, L::Stack, S> {
        crate::middleware::stack::sealed::Apply::apply(chain, self)
    }

    /// Runs each covered operation's guard at this point of the group's chain,
    /// from the request head: inside the interceptors applied before this
    /// call, and outside those applied after it.
    ///
    /// So an anonymous caller is refused before a byte of its body is read:
    /// called before a [`BodySize`](crate::middleware::limits::body_size::BodySize),
    /// the guard's 401 is answered without buffering what that limit allows.
    /// The handler's guard argument then receives the credential already
    /// checked, so nothing is checked twice.
    ///
    /// Only *when* the guard runs moves. It is declared by the handler's
    /// argument exactly as before, so the description is unchanged, and an
    /// operation taking no guard is left as it was. A CORS preflight runs no
    /// interceptor and so no guard; a `Cors` meant to cover the guard's
    /// refusal is applied before this call.
    ///
    /// A guard runs once. Where an enclosing scope also called this —
    /// [`Router::guard_first`](crate::router::Router::guard_first), or a
    /// router this group's is nested in — the outermost point wins; a second
    /// call here keeps the first.
    ///
    /// ```no_run
    /// use kynos::{middleware::limits::body_size::BodySize, router::group::Group};
    ///
    /// let uploads = Group::<()>::new("/uploads")
    ///     .guard_first()
    ///     .intercept(BodySize::new(96 << 20));
    /// # let _ = uploads;
    /// ```
    #[must_use]
    pub fn guard_first(mut self) -> Self {
        self.guard_first.get_or_insert(self.interceptors.len());
        self
    }

    /// Mounts operations into this group.
    ///
    /// What the endpoints carry is checked against the group's own
    /// interceptors and then remembered, so a later [`intercept`] sees it. It
    /// is *not* checked against an earlier `mount`, since no request reaches
    /// two operations.
    ///
    /// Mounting operations that carry no interceptor leaves this type
    /// unchanged.
    ///
    /// # Panics
    ///
    /// Panics when `endpoints` contains, directly or inside a tuple, array or
    /// `Vec`, an `assets` feature `AssetSet` holding an asset whose path is not
    /// a legal path template, which only a hand-built `Asset` can carry.
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

/// The statuses a [`kynos_openapi::Responses`] value declares as exact codes.
///
/// Wildcard patterns and `default` are skipped: they name no one status.
#[must_use]
fn described_statuses(responses: &kynos_openapi::Responses) -> Vec<u16> {
    responses
        .responses
        .keys()
        .filter_map(|key| key.parse::<u16>().ok())
        .collect()
}

/// Checks that a [`ShortCircuit`]'s const and its responses agree.
///
/// Returns the violation when they do not. Called while a router or a group
/// is built, where a [`Registry`] exists, for each interceptor either takes.
#[must_use]
pub(crate) fn short_circuit_mismatch<S: ShortCircuit>(
    registry: &mut Registry,
) -> Option<RouterError> {
    mismatch_between(
        std::any::type_name::<S>(),
        S::STATUSES,
        &S::responses(registry),
    )
}

/// The comparison itself, without the type parameter, for testing.
fn mismatch_between(
    name: &str,
    statuses: &[u16],
    responses: &kynos_openapi::Responses,
) -> Option<RouterError> {
    let normalize = |mut codes: Vec<u16>| {
        codes.sort_unstable();
        codes.dedup();
        codes
    };

    let declared = normalize(statuses.to_vec());
    let described = normalize(described_statuses(responses));

    if declared == described {
        return None;
    }

    Some(RouterError::ShortCircuitMismatch {
        name: name.to_owned(),
        declared,
        described,
    })
}

#[cfg(test)]
mod tests;
