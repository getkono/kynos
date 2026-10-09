//! The type-level list of interceptors covering a route, and the check that
//! rejects two of them colliding.
//!
//! Interceptors are erased for execution; alongside rides a phantom list of
//! their types, carried by [`Router`](crate::router::Router),
//! [`Group`](crate::router::group::Group) and
//! [`EndpointBuilder`](crate::router::endpoint::builder::EndpointBuilder), so
//! that mounting an interceptor that would collide with one already there
//! fails to compile. Nothing here exists at run time.
//!
//! # What counts as a collision
//!
//! Two interceptors covering one operation collide when they both add the same
//! response header, or both answer with the same status. Reading the same
//! request header is *not* a collision.

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
/// builds for a `routes![a, b]`: each side is checked against the router's
/// stack, never against the other, since no request reaches both.
#[derive(Debug)]
pub struct Both<L, R>(PhantomData<fn() -> (L, R)>);

/// Whether two ASCII strings match, ignoring case.
///
/// Header names are case-insensitive per RFC 9110 section 5.1.
#[must_use]
pub(crate) const fn header_name_eq(left: &str, right: &str) -> bool {
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
