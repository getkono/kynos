//! The type-level list of interceptors covering a route, and the check that
//! rejects two of them colliding.
//!
//! Interceptors are erased for execution; alongside rides a phantom list of
//! their types, carried by [`Router`](crate::router::Router),
//! [`Group`] and
//! [`EndpointBuilder`](crate::router::endpoint::builder::EndpointBuilder), so
//! that mounting an interceptor that would collide with one already there
//! fails to compile. Nothing here exists at run time save
//! [`InterceptorList`], which is a tuple of `intercept` calls.
//!
//! # What counts as a collision
//!
//! Two interceptors covering one operation collide when they both add the same
//! response header, or both short-circuit with the same status. Reading the
//! same request header is *not* a collision, and neither is the 400 a fallible
//! [`Interceptor::Reads`] answers when its headers fail to decode: that status
//! is never compared, since it is the one a handler's own extractors share.
//!
//! Two interceptors are compared only where the methods they cover overlap. A
//! [`When<M, N>`](When) covers the methods its [`MethodSet`] `M` names, and
//! every other interceptor covers all of them, so two scoped to disjoint sets
//! are never compared: no operation runs both. Two whose sets overlap are
//! compared even where the group declares no operation with a shared method,
//! so mounting one later cannot turn a compiling group into a colliding one.

use std::{future::Future, marker::PhantomData};

use crate::{
    extract::params::header::HeaderParams,
    http::{Request, header_name_eq},
    middleware::{Continued, Interceptor, Next, Placement, catch_panic::PanicPolicy},
    response::ShortCircuit,
    router::group::{Group, MethodSet, Methods},
};

/// One interceptor in front of the rest.
///
/// The empty stack is `()`. Written by hand it reads innermost first, the
/// reverse of the calls that built it; [`stack!`](crate::stack!) spells it in
/// mount order instead.
#[derive(Debug)]
pub struct Cons<H, T>(PhantomData<fn() -> (H, T)>);

/// The stack element [`Group::intercept_when`] puts in front: `N`, covering only
/// the operations whose method the [`MethodSet`] `M` names.
///
/// A type and never a value: it is uninhabited, and exists so that a group's
/// type still names every interceptor it holds, which `stack!` spells as
/// `When<M, N>`. It declares exactly what `N` does, and the collision check
/// compares it only against the interceptors whose methods overlap `M`'s.
///
/// ```no_run
/// use kynos::{
///     middleware::{
///         catch_panic::Propagate, limits::body_size::BodySize, request_id::RequestId,
///         stack::When,
///     },
///     router::group::{Group, UnsafeMethods},
/// };
///
/// fn users() -> Group<(), Propagate, kynos::stack![RequestId, When<UnsafeMethods, BodySize>]> {
///     Group::new("/users")
///         .intercept(RequestId::new())
///         .intercept_when(UnsafeMethods, BodySize::new(64 * 1024))
/// }
/// # let _ = users();
/// ```
pub struct When<M, N> {
    never: std::convert::Infallible,
    _interceptor: PhantomData<fn() -> (M, N)>,
}

impl<M, N> std::fmt::Debug for When<M, N> {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.never {}
    }
}

impl<C, M, N> Interceptor<C> for When<M, N>
where
    C: Sync + 'static,
    M: MethodSet,
    N: Interceptor<C>,
{
    type Reads = N::Reads;
    type Adds = N::Adds;
    type Short = N::Short;

    const ADDS_ON: Placement = N::ADDS_ON;

    const __METHODS: Methods = M::METHODS;

    fn intercept(
        &self,
        _: Request,
        _: Self::Reads,
        _: &C,
        _: Next<'_, C>,
    ) -> impl Future<Output = Result<Continued<Self::Adds>, Self::Short>> + Send {
        // Never called: there is no `self` to call it on. `N` itself is what
        // the group holds and runs.
        std::future::pending()
    }
}

/// The stack type a chain of `intercept` calls builds, written in call order.
///
/// Each `intercept` puts its interceptor in front of the stack's type, so the
/// [`Cons`] list reads innermost first. This takes the interceptor types in the
/// order they are mounted, outermost first, and expands to that list, for the
/// places the type has to be named: chiefly a function returning a
/// [`Group`] that is still to be mounted into, or intercepted further.
///
/// `stack![]` is the empty stack, `()`.
///
/// ```no_run
/// use std::time::Duration;
///
/// use kynos::{
///     middleware::{
///         catch_panic::Propagate,
///         limits::{body_size::BodySize, timeout::Timeout},
///         request_id::RequestId,
///     },
///     router::group::Group,
/// };
///
/// fn writes(prefix: &str) -> Group<(), Propagate, kynos::stack![RequestId, Timeout, BodySize]> {
///     Group::new(prefix).intercept_all((
///         RequestId::new(),
///         Timeout::new(Duration::from_secs(5)),
///         BodySize::new(64 * 1024),
///     ))
/// }
/// # let _ = writes("/users");
/// ```
///
/// The order is the type's: `stack![A, B]` and `stack![B, A]` are different
/// types, as `intercept(a).intercept(b)` and `intercept(b).intercept(a)` are.
#[macro_export]
macro_rules! stack {
    (@onto $stack:ty;) => { $stack };
    (@onto $stack:ty; $head:ty $(, $rest:ty)*) => {
        $crate::stack!(@onto $crate::middleware::stack::Cons<$head, $stack>; $($rest),*)
    };
    ($($interceptor:ty),* $(,)?) => { $crate::stack!(@onto (); $($interceptor),*) };
}

/// Interceptors applied together by
/// [`Group::intercept_all`], first element outermost.
///
/// Implemented for tuples of up to sixteen interceptors, and sealed. `I` is the
/// stack already mounted and `S` what the group's endpoints brought: each
/// element is checked against both and against the elements before it, by the
/// same [`CompatibleWith`] a single `intercept` uses, since applying the tuple
/// *is* that sequence of calls.
pub trait InterceptorList<C, I, S>: sealed::Apply<C, I, S> {
    /// The stack once every element is mounted: `I` with each element in front
    /// of it, last element first.
    type Stack;
}

pub(crate) mod sealed {
    use super::InterceptorList;
    use crate::{middleware::catch_panic::PanicPolicy, router::group::Group};

    /// The one operation an [`InterceptorList`] has, unreachable from outside
    /// the crate so that the tuples are its only implementors.
    pub trait Apply<C, I, S> {
        /// `intercept` for each element, in order.
        fn apply<P: PanicPolicy>(
            self,
            group: Group<C, P, I, S>,
        ) -> Group<C, P, <Self as InterceptorList<C, I, S>>::Stack, S>
        where
            Self: InterceptorList<C, I, S>;
    }
}

impl<C, I, S> InterceptorList<C, I, S> for () {
    type Stack = I;
}

impl<C, I, S> sealed::Apply<C, I, S> for () {
    fn apply<P: PanicPolicy>(
        self,
        group: Group<C, P, I, S>,
    ) -> Group<C, P, <Self as InterceptorList<C, I, S>>::Stack, S> {
        group
    }
}

/// One tuple arity, defined by the arity below it: the head is intercepted, and
/// the tail is a list over the stack that leaves.
macro_rules! interceptor_list {
    ($head:ident $(, $tail:ident)*) => {
        impl<C, I, S, $head, $($tail),*> InterceptorList<C, I, S> for ($head, $($tail,)*)
        where
            C: Sync + 'static,
            $head: Interceptor<C>,
            I: CompatibleWith<$head, C>,
            S: CompatibleWith<$head, C>,
            ($($tail,)*): InterceptorList<C, Cons<$head, I>, S>,
        {
            type Stack = <($($tail,)*) as InterceptorList<C, Cons<$head, I>, S>>::Stack;
        }

        impl<C, I, S, $head, $($tail),*> sealed::Apply<C, I, S> for ($head, $($tail,)*)
        where
            C: Sync + 'static,
            $head: Interceptor<C>,
            I: CompatibleWith<$head, C>,
            S: CompatibleWith<$head, C>,
            ($($tail,)*): InterceptorList<C, Cons<$head, I>, S>,
        {
            #[allow(non_snake_case)]
            fn apply<P: PanicPolicy>(
                self,
                group: Group<C, P, I, S>,
            ) -> Group<C, P, <Self as InterceptorList<C, I, S>>::Stack, S> {
                let ($head, $($tail,)*) = self;
                sealed::Apply::apply(($($tail,)*), group.intercept($head))
            }
        }
    };
}

interceptor_list!(T1);
interceptor_list!(T1, T2);
interceptor_list!(T1, T2, T3);
interceptor_list!(T1, T2, T3, T4);
interceptor_list!(T1, T2, T3, T4, T5);
interceptor_list!(T1, T2, T3, T4, T5, T6);
interceptor_list!(T1, T2, T3, T4, T5, T6, T7);
interceptor_list!(T1, T2, T3, T4, T5, T6, T7, T8);
interceptor_list!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
interceptor_list!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
interceptor_list!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
interceptor_list!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
interceptor_list!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
interceptor_list!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
interceptor_list!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
);
interceptor_list!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16
);

/// Two stacks that cover different operations.
///
/// What [`IntoEndpoints::Stacks`](crate::router::endpoint::set::IntoEndpoints::Stacks)
/// builds for a `routes![a, b]`: each side is checked against the router's
/// stack, never against the other, since no request reaches both.
#[derive(Debug)]
pub struct Both<L, R>(PhantomData<fn() -> (L, R)>);

/// Whether two header-name lists share nothing.
#[must_use]
pub(crate) const fn header_names_disjoint(left: &[&str], right: &[&str]) -> bool {
    let mut outer = 0;
    while outer < left.len() {
        let mut inner = 0;
        while inner < right.len() {
            if header_name_eq(left[outer], right[inner]) {
                return false;
            }
            inner += 1;
        }
        outer += 1;
    }

    true
}

/// Whether two status lists share nothing.
#[must_use]
pub(crate) const fn statuses_disjoint(left: &[u16], right: &[u16]) -> bool {
    let mut outer = 0;
    while outer < left.len() {
        let mut inner = 0;
        while inner < right.len() {
            if left[outer] == right[inner] {
                return false;
            }
            inner += 1;
        }
        outer += 1;
    }

    true
}

/// One stack folded onto another, with the empty stack erased.
///
/// A router remembers what it has mounted so that an `intercept` written
/// afterwards is still checked against it.
///
/// Mounting interceptor-free operations leaves a router's type untouched, so
/// re-assignment and conditional mounting keep working. Flattening siblings
/// into one `Cons` list is sound because no check compares two members of one
/// list to each other.
pub trait Flatten<S> {
    /// The two stacks as one list.
    type Out;
}

impl<S> Flatten<S> for () {
    type Out = S;
}

impl<H, T, S> Flatten<S> for Cons<H, T>
where
    T: Flatten<S>,
{
    type Out = Cons<H, <T as Flatten<S>>::Out>;
}

impl<L, R, S> Flatten<S> for Both<L, R>
where
    R: Flatten<S>,
    L: Flatten<<R as Flatten<S>>::Out>,
{
    type Out = <L as Flatten<<R as Flatten<S>>::Out>>::Out;
}

/// Fails to evaluate where `N` claims to cover fewer than every method, which
/// only a [`When`] may: each site mounting an interceptor value forces it, so
/// the collision check never narrows what an interceptor runs on.
pub(crate) const fn unscoped<C, N>()
where
    C: Sync + 'static,
    N: Interceptor<C>,
{
    assert!(
        N::__METHODS.is_every(),
        "an interceptor covers every operation it is mounted over; \
         scope it to methods with `Group::intercept_when`"
    );
}

/// A stack that does not collide with the interceptor `N`.
///
/// Implemented for every stack; [`CHECK`] is a `const` that fails to evaluate
/// when two interceptors collide, forced at the mount site.
///
/// [`CHECK`]: CompatibleWith::CHECK
pub trait CompatibleWith<N, C> {
    /// Evaluates to nothing, or fails to evaluate at all.
    const CHECK: ();
}

impl<N, C> CompatibleWith<N, C> for () {
    const CHECK: () = ();
}

impl<N, H, T, C> CompatibleWith<N, C> for Cons<H, T>
where
    C: Sync + 'static,
    N: Interceptor<C>,
    H: Interceptor<C>,
    T: CompatibleWith<N, C>,
{
    const CHECK: () = {
        // Disjoint methods cover no operation in common.
        if N::__METHODS.overlaps(H::__METHODS) {
            assert!(
                header_names_disjoint(
                    <N::Adds as HeaderParams>::NAMES,
                    <H::Adds as HeaderParams>::NAMES,
                ),
                "two interceptors covering this route add the same response header; \
                 mount them at different scopes, or have one of them stop adding it"
            );

            assert!(
                statuses_disjoint(
                    <N::Short as ShortCircuit>::STATUSES,
                    <H::Short as ShortCircuit>::STATUSES,
                ),
                "two interceptors covering this route answer with the same status; \
                 a consumer could not tell which one replied"
            );
        }

        let () = <T as CompatibleWith<N, C>>::CHECK;
    };
}

impl<N, L, R, C> CompatibleWith<N, C> for Both<L, R>
where
    L: CompatibleWith<N, C>,
    R: CompatibleWith<N, C>,
{
    const CHECK: () = {
        let () = <L as CompatibleWith<N, C>>::CHECK;
        let () = <R as CompatibleWith<N, C>>::CHECK;
    };
}

/// A stack that does not collide with any interceptor in `Other`.
///
/// The cross-product of [`CompatibleWith`], for the places two whole stacks
/// meet: mounting a group into a router, nesting, merging, and mounting
/// endpoints that carry interceptors of their own.
pub trait CompatibleStack<Other, C> {
    /// Evaluates to nothing, or fails to evaluate at all.
    const CHECK: ();
}

impl<Other, C> CompatibleStack<Other, C> for () {
    const CHECK: () = ();
}

impl<Other, H, T, C> CompatibleStack<Other, C> for Cons<H, T>
where
    Other: CompatibleWith<H, C>,
    T: CompatibleStack<Other, C>,
{
    const CHECK: () = {
        let () = <Other as CompatibleWith<H, C>>::CHECK;
        let () = <T as CompatibleStack<Other, C>>::CHECK;
    };
}

impl<Other, L, R, C> CompatibleStack<Other, C> for Both<L, R>
where
    L: CompatibleStack<Other, C>,
    R: CompatibleStack<Other, C>,
{
    const CHECK: () = {
        let () = <L as CompatibleStack<Other, C>>::CHECK;
        let () = <R as CompatibleStack<Other, C>>::CHECK;
    };
}

#[cfg(test)]
mod tests;
