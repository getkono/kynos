use std::convert::Infallible;

use super::{Both, Cons, Flatten, InterceptorList, header_names_disjoint, statuses_disjoint};
use crate::{
    http::Request,
    middleware::{Continued, Interceptor, Next, catch_panic::Propagate},
    router::group::Group,
};

/// An interceptor that collides with nothing, one distinct type per `N`.
struct Noop<const N: u8>;

impl<const N: u8> Interceptor<()> for Noop<N> {
    type Reads = ();
    type Adds = ();
    type Short = Infallible;

    async fn intercept(
        &self,
        request: Request,
        reads: (),
        context: &(),
        next: Next<'_, ()>,
    ) -> Result<Continued, Infallible> {
        let _ = (reads, context);
        Ok(next.run(request).await)
    }
}

/// Two types are one. Checked while this compiles, so it has no body.
fn same<A: Is<B>, B>() {}

trait Is<T> {}
impl<T> Is<T> for T {}

/// `stack!` is written outermost first and expands innermost first.
#[test]
fn a_stack_is_named_in_mount_order() {
    struct Outer;
    struct Middle;
    struct Inner;

    same::<crate::stack![], ()>();
    same::<crate::stack![Outer], Cons<Outer, ()>>();
    same::<crate::stack![Outer, Middle, Inner,], Cons<Inner, Cons<Middle, Cons<Outer, ()>>>>();
}

/// A tuple's stack is the one its elements' `intercept` calls build, laid on
/// top of what was already mounted.
#[test]
fn a_tuple_stacks_its_elements_in_order_onto_the_existing_stack() {
    type Mounted = Cons<Noop<9>, ()>;

    same::<<(Noop<0>,) as InterceptorList<(), (), ()>>::Stack, crate::stack![Noop<0>]>();
    same::<
        <(Noop<0>, Noop<1>) as InterceptorList<(), Mounted, ()>>::Stack,
        crate::stack![Noop<9>, Noop<0>, Noop<1>],
    >();
    same::<<() as InterceptorList<(), Mounted, ()>>::Stack, Mounted>();
}

/// `intercept_all` returns the group `intercept` per element would, at both
/// ends of the arity list.
#[test]
fn intercept_all_is_the_sequence_of_intercept_calls() {
    type Sixteen = crate::stack![
        Noop<0>,
        Noop<1>,
        Noop<2>,
        Noop<3>,
        Noop<4>,
        Noop<5>,
        Noop<6>,
        Noop<7>,
        Noop<8>,
        Noop<9>,
        Noop<10>,
        Noop<11>,
        Noop<12>,
        Noop<13>,
        Noop<14>,
        Noop<15>,
    ];

    let one: Group<(), Propagate, crate::stack![Noop<0>]> =
        Group::new("/a").intercept_all((Noop::<0>,));
    let chained: Group<(), Propagate, crate::stack![Noop<0>, Noop<1>]> =
        Group::new("/a").intercept(Noop::<0>).intercept(Noop::<1>);
    let all: Group<(), Propagate, crate::stack![Noop<0>, Noop<1>]> =
        Group::new("/a").intercept_all((Noop::<0>, Noop::<1>));
    let widest: Group<(), Propagate, Sixteen> = Group::new("/a").intercept_all((
        Noop::<0>, Noop::<1>, Noop::<2>, Noop::<3>, Noop::<4>, Noop::<5>, Noop::<6>, Noop::<7>,
        Noop::<8>, Noop::<9>, Noop::<10>, Noop::<11>, Noop::<12>, Noop::<13>, Noop::<14>,
        Noop::<15>,
    ));

    let _ = (one, chained, all, widest);
}

#[test]
fn a_shared_header_is_not_disjoint() {
    assert!(header_names_disjoint(&["x-a"], &["x-b", "x-c"]));
    assert!(!header_names_disjoint(&["x-a", "x-b"], &["X-B"]));
    assert!(header_names_disjoint(&[], &["x-a"]));
}

#[test]
fn a_shared_status_is_not_disjoint() {
    assert!(statuses_disjoint(&[413], &[503, 504]));
    assert!(!statuses_disjoint(&[429, 503], &[503]));

    // An interceptor that never short-circuits collides with nothing,
    // which is what lets any number of them compose.
    assert!(statuses_disjoint(&[], &[503]));
}

#[test]
fn the_checks_are_usable_in_const_context() {
    // The whole point: these run while the program is compiled, which is
    // what lets `Router::intercept` reject a collision there rather than
    // when the router is built.
    const {
        assert!(header_names_disjoint(&["x-a"], &["x-b"]));
        assert!(statuses_disjoint(&[413], &[504]));
        assert!(!header_names_disjoint(&["x-a"], &["X-A"]));
    }
}

/// An empty stack folds away rather than accumulating.
///
/// The property `Router::mount` rests on: `routes![a]` carries `()` and
/// `routes![b, c]` carries `Both<(), ()>`, so mounting operations that hold no
/// interceptor has to leave the router's type exactly as it was. Asserted by
/// type equality, which is checked while this compiles -- `flattens_to` has no
/// body because there is nothing to run.
#[test]
fn an_empty_stack_flattens_away() {
    fn flattens_to<A: Flatten<S, Out = B>, B, S>() {}

    struct Left;
    struct Right;

    flattens_to::<(), (), ()>();
    flattens_to::<Both<(), ()>, (), ()>();
    flattens_to::<Both<Both<(), ()>, ()>, (), ()>();

    // A carried stack survives, and lands in front of what was already there.
    flattens_to::<Cons<Left, ()>, Cons<Left, ()>, ()>();
    flattens_to::<Cons<Left, ()>, Cons<Left, Cons<Right, ()>>, Cons<Right, ()>>();

    // `Both` concatenates rather than nesting, so two mounted scopes become
    // one list for a later interceptor to be compared against.
    flattens_to::<Both<Cons<Left, ()>, Cons<Right, ()>>, Cons<Left, Cons<Right, ()>>, ()>();
}
