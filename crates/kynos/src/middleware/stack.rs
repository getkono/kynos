//! The type-level list of interceptors covering a route, and the check that
//! rejects two of them colliding.
//!
//! Interceptors are erased for *execution* — a router holds a chain it cannot
//! name, and [`Next`](crate::middleware::Next) keeps its two parameters. What
//! rides alongside is a phantom list of their types, carried by
//! [`Router`](crate::router::Router), [`Group`](crate::router::group::Group)
//! and [`EndpointBuilder`](crate::router::endpoint::builder::EndpointBuilder)
//! so that mounting an interceptor that would collide with one already there
//! fails to compile.
//!
//! Nothing here exists at run time. `Cons` and `Both` are uninhabitable
//! markers, and every check is a `const` evaluated while the program is
//! compiled.
//!
//! # What counts as a collision
//!
//! Two interceptors covering one operation collide when they both add the same
//! response header, or both answer with the same status. Either would leave a
//! consumer unable to tell which one spoke.
//!
//! Reading the same request header is *not* a collision: reading is
//! non-destructive, two interceptors may both look at `X-Tenant`, and the
//! operation declares the parameter once. Rejecting that would block a
//! third-party interceptor from reading a header a shipped one already reads,
//! for no gain.

use std::marker::PhantomData;

use crate::{
    extract::params::header::HeaderParams, middleware::Interceptor, response::ShortCircuit,
};

/// One interceptor in front of the rest.
///
/// The empty stack is `()`.
#[derive(Debug)]
pub struct Cons<H, T>(PhantomData<fn() -> (H, T)>);

/// Two stacks that cover different operations.
///
/// What [`IntoEndpoints::Stacks`](crate::router::endpoint::set::IntoEndpoints::Stacks)
/// builds for a `routes![a, b]`: each endpoint's own stack is checked against
/// the router's, and never against the other endpoint's — two operations cannot
/// collide with each other, since no request reaches both.
#[derive(Debug)]
pub struct Both<L, R>(PhantomData<fn() -> (L, R)>);

/// Whether two ASCII strings match, ignoring case.
///
/// Header names are case-insensitive per RFC 9110, so `X-Request-Id` and
/// `x-request-id` are one name and must collide.
#[must_use]
pub const fn header_name_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());

    if left.len() != right.len() {
        return false;
    }

    let mut index = 0;
    while index < left.len() {
        if !left[index].eq_ignore_ascii_case(&right[index]) {
            return false;
        }
        index += 1;
    }

    true
}

/// Whether two header-name lists share nothing.
#[must_use]
pub const fn header_names_disjoint(left: &[&str], right: &[&str]) -> bool {
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
pub const fn statuses_disjoint(left: &[u16], right: &[u16]) -> bool {
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
/// What a scope's interceptors become when the scope is mounted: a router
/// remembers what it has mounted so that an `intercept` written *afterwards*
/// is checked against it, since those interceptors cover the same operations
/// at run time whichever order the two calls were written in.
///
/// The collapse is the point. `<() as Flatten<S>>::Out` and
/// `<Both<(), ()> as Flatten<S>>::Out` are both `S`, so mounting
/// interceptor-free operations leaves a router's type untouched and
/// re-assignment and conditional mounting keep working. Only mounting
/// something that carries an interceptor changes the type — which `intercept`
/// already does.
///
/// The result is a `Cons` list, and that is sound because nothing ever
/// compares two members of one list to each other: [`CompatibleWith`] compares
/// a newcomer against each member, and [`CompatibleStack`] compares another
/// stack against each member. Two sibling scopes therefore stay compatible,
/// correctly — no request reaches both.
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

/// A stack that does not collide with the interceptor `N`.
///
/// Implemented for every stack; the obligation lives in [`CHECK`], which is a
/// `const` that fails to evaluate when two interceptors collide. `Router` and
/// `Group` force it at the mount site, so the error lands on the call rather
/// than somewhere in this module.
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
