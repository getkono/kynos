use super::{Both, Cons, Flatten, header_name_eq, header_names_disjoint, statuses_disjoint};

#[test]
fn header_names_compare_without_regard_to_case() {
    assert!(header_name_eq("X-Request-Id", "x-request-id"));
    assert!(!header_name_eq("x-request-id", "x-request-ids"));
    assert!(!header_name_eq("x-a", "x-b"));
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
